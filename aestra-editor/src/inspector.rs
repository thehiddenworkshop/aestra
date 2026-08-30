//! Inspector ownership: module-stack UI, semantic property editing, numeric scrubbing,
//! navigation focus, and contextual help.

use crate::feathers::panel_card::{
    PanelCardProps, RememberedPanelCard, spawn_panel_card as spawn_remembered_panel_card,
};
use crate::feathers::slider_row::{SliderNumberInputPair, SliderRowProps, spawn_slider_input_pair};
use crate::timeline::{
    EffectClipChildSelection, EffectClipPath, TimelineState, resolve_effect_clip_path,
};
use crate::*;
use aestra_bevy::{EffectClip, EffectClipId};
use aestra_compiler::{InputControl, InputMetadata, ModuleRegistry};
use bevy::{
    ui::InteractionDisabled,
    ui_widgets::{Activate, SliderValue},
};
use fluent_bundle::FluentArgs;

pub(crate) const INSPECTOR_HIGHLIGHT_DURATION: f32 = 1.6;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InspectorSet {
    Input,
    Actions,
    Sync,
}

pub(crate) struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorModuleRegistry>()
            .init_resource::<ModulePaletteState>()
            .init_resource::<EffectClipRepairState>()
            .init_resource::<InspectorFocus>()
            .init_resource::<NumericScrubState>()
            .init_resource::<BoundedSliderState>()
            .add_observer(queue_inspector_action_activation)
            .add_observer(handle_document_text_change)
            .add_observer(handle_document_toggle_change)
            .add_observer(handle_emitter_capacity_change)
            .add_observer(handle_inspector_toggle_change)
            .add_observer(handle_module_enabled_change)
            .add_observer(handle_renderer_enabled_change)
            .add_observer(handle_renderer_scalar_change)
            .add_observer(handle_renderer_toggle_change)
            .add_observer(handle_emitter_scalar_change)
            .add_observer(handle_effect_clip_scalar_change)
            .add_observer(update_effect_clip_repair_query)
            .add_observer(handle_inspector_integer_change)
            .add_observer(handle_inspector_scalar_change)
            .add_observer(handle_bounded_slider_change)
            .add_observer(begin_numeric_scrub)
            .add_observer(update_numeric_scrub)
            .add_observer(finish_numeric_scrub)
            .add_observer(select_inspector_header)
            .add_systems(Update, module_palette_keyboard.in_set(InspectorSet::Input))
            .add_systems(
                Update,
                handle_inspector_actions.in_set(InspectorSet::Actions),
            )
            .add_systems(
                Update,
                (
                    (
                        sync_emitter_capacity_inputs,
                        sync_emitter_number_inputs,
                        sync_effect_clip_number_inputs,
                        sync_inspector_number_inputs,
                        sync_inspector_slider_inputs,
                        sync_renderer_number_inputs,
                        sync_renderer_slider_inputs,
                        sync_effect_clip_inspector_timing,
                        sync_effect_clip_repair_candidates,
                    )
                        .chain(),
                    scroll_inspector_to_focus,
                    update_inspector_highlight,
                    decorate_numeric_scrub_inputs,
                )
                    .chain()
                    .in_set(InspectorSet::Sync),
            );
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) enum InspectorAction {
    OpenModulePalette(StackStage),
    CloseModulePalette,
    AddModule(usize),
    AddSpriteRenderer,
    AddFlipbookRenderer,
    MoveModule(ModuleId, i8),
    DuplicateModule(ModuleId),
    DeleteModule(ModuleId),
    DuplicateRenderer(RendererId),
    DeleteRenderer(RendererId),
    ToggleSection(InspectorSection),
    SetModuleChoice {
        module: ModuleId,
        input: u8,
        choice: u8,
    },
    SetRendererMaterial(RendererId, usize),
    SetRendererBlend(RendererId, BlendMode),
    SetRendererTexture(RendererId, Option<usize>),
    SetRendererFlipbook(RendererId, usize),
    SetFlipbookTimeSource(RendererId, FlipbookTimeSource),
    SetFlipbookPlayback(RendererId, FlipbookPlaybackMode),
    AddEventLink {
        trigger: EventTrigger,
        target: EmitterId,
    },
    DeleteEventLink(EventId),
    RepairEffectClipSource {
        clip: EffectClipId,
        source: EffectAssetRef,
    },
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EffectClipRepairState {
    query: String,
}

#[derive(Component)]
struct EffectClipRepairSearchInput;

#[derive(Component, Debug, Clone)]
struct EffectClipRepairCandidate {
    search_text: String,
}

#[derive(Component)]
struct EffectClipRepairEmpty;

#[derive(Debug, Clone, PartialEq, Eq)]
enum InspectorStatus {
    SelectedCompiled(String),
    Selected(String),
    ModuleRegistryUnavailable,
    ModuleMissing,
    InputMetadataUnavailable,
    NotChoice(String),
    ChoiceUnavailable,
    TargetUnavailable,
    FiniteNumberRequired(String),
    IncompatibleMetadata(String),
    Updated(String),
    NameRequired(String),
    EventAdded { trigger: String, target: String },
    EventRemoved,
    EventDuplicate,
    EventSelfTarget,
    EventTargetMissing,
    RepairRejected(String),
}

fn set_inspector_status(
    session: &mut EditorSession,
    localizer: &Localizer,
    status: InspectorStatus,
) {
    session.status = localize_inspector_status(status, localizer);
}

fn localize_inspector_status(status: InspectorStatus, localizer: &Localizer) -> String {
    let (message_id, argument) = match status {
        InspectorStatus::SelectedCompiled(target) => {
            ("inspector-status-selected-compiled", ("target", target))
        }
        InspectorStatus::Selected(target) => ("inspector-status-selected", ("target", target)),
        InspectorStatus::ModuleRegistryUnavailable => {
            return localizer.text("inspector-status-module-registry-unavailable");
        }
        InspectorStatus::ModuleMissing => {
            return localizer.text("inspector-status-module-missing");
        }
        InspectorStatus::InputMetadataUnavailable => {
            return localizer.text("inspector-status-input-metadata-unavailable");
        }
        InspectorStatus::NotChoice(input) => ("inspector-status-not-choice", ("input", input)),
        InspectorStatus::ChoiceUnavailable => {
            return localizer.text("inspector-status-choice-unavailable");
        }
        InspectorStatus::TargetUnavailable => {
            return localizer.text("inspector-status-target-unavailable");
        }
        InspectorStatus::FiniteNumberRequired(parameter) => (
            "inspector-status-finite-number-required",
            ("parameter", parameter),
        ),
        InspectorStatus::IncompatibleMetadata(parameter) => (
            "inspector-status-incompatible-metadata",
            ("parameter", parameter),
        ),
        InspectorStatus::Updated(target) => ("inspector-status-updated", ("target", target)),
        InspectorStatus::NameRequired(target) => {
            ("inspector-status-name-required", ("target", target))
        }
        InspectorStatus::EventAdded { trigger, target } => {
            let mut args = FluentArgs::new();
            args.set("trigger", trigger);
            args.set("target", target);
            return localizer.text_with("inspector-status-event-added", &args);
        }
        InspectorStatus::EventRemoved => {
            return localizer.text("inspector-status-event-removed");
        }
        InspectorStatus::EventDuplicate => {
            return localizer.text("inspector-status-event-duplicate");
        }
        InspectorStatus::EventSelfTarget => {
            return localizer.text("inspector-status-event-self-target");
        }
        InspectorStatus::EventTargetMissing => {
            return localizer.text("inspector-status-event-target-missing");
        }
        InspectorStatus::RepairRejected(reason) => {
            let mut args = FluentArgs::new();
            args.set("reason", reason);
            return localizer.text_with("inspector-repair-rejected", &args);
        }
    };
    let mut args = FluentArgs::new();
    args.set(argument.0, argument.1);
    localizer.text_with(message_id, &args)
}

fn queue_inspector_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<InspectorAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

fn update_effect_clip_repair_query(
    change: On<ValueChange<String>>,
    inputs: Query<(), With<EffectClipRepairSearchInput>>,
    mut state: ResMut<EffectClipRepairState>,
) {
    if inputs.contains(change.source) && state.query != change.value {
        state.query.clone_from(&change.value);
    }
}

fn sync_effect_clip_repair_candidates(
    state: Res<EffectClipRepairState>,
    mut candidates: Query<(&EffectClipRepairCandidate, &mut Node)>,
    mut empty_states: Query<
        &mut Node,
        (
            With<EffectClipRepairEmpty>,
            Without<EffectClipRepairCandidate>,
        ),
    >,
) {
    let query = state.query.trim().to_lowercase();
    let mut visible = 0;
    for (candidate, mut node) in &mut candidates {
        let matches = query.is_empty() || candidate.search_text.contains(&query);
        node.display = if matches {
            visible += 1;
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut empty_states {
        node.display = if visible == 0 {
            Display::Flex
        } else {
            Display::None
        };
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn handle_inspector_actions(
    mut commands: Commands,
    mut actions: Query<
        (
            Entity,
            &Interaction,
            &InspectorAction,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            Option<&InteractionDisabled>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut session: ResMut<EditorSession>,
    mut menu: ResMut<MenuState>,
    registry: Res<EditorModuleRegistry>,
    mut palette: ResMut<ModulePaletteState>,
    mut workspace: ResMut<CurvesState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut settings: ResMut<EditorSettings>,
    mut settings_persistence: ResMut<SettingsPersistence>,
    localizer: Res<Localizer>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    mut repair: Option<ResMut<EffectClipRepairState>>,
) {
    for (entity, interaction, action, feathers_action, pending, disabled, mut background) in
        &mut actions
    {
        if disabled.is_some() {
            if feathers_action.is_none() {
                background.0 = theme::PANEL_DARK;
            }
            continue;
        }
        match *interaction {
            Interaction::Hovered if feathers_action.is_none() => {
                background.0 = theme::BUTTON_HOVER;
            }
            Interaction::None if feathers_action.is_none() => {
                background.0 = theme::BUTTON;
            }
            Interaction::Pressed => {
                if feathers_action.is_some() {
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
                match *action {
                    InspectorAction::OpenModulePalette(stage) => {
                        palette.open = true;
                        palette.stage = stage;
                        palette.query.clear();
                        session.ui_revision += 1;
                    }
                    InspectorAction::CloseModulePalette => {
                        palette.open = false;
                        session.ui_revision += 1;
                    }
                    InspectorAction::AddModule(index) => {
                        let module = registry
                            .0
                            .iter()
                            .nth(index)
                            .and_then(|metadata| registry.0.instantiate(&metadata.type_id));
                        if let Some(module) = module {
                            session.add_module(module);
                            palette.open = false;
                        } else {
                            set_inspector_status(
                                &mut session,
                                &localizer,
                                InspectorStatus::ModuleRegistryUnavailable,
                            );
                        }
                    }
                    InspectorAction::AddSpriteRenderer => {
                        session.add_sprite_renderer();
                        palette.open = false;
                    }
                    InspectorAction::AddFlipbookRenderer => {
                        session.add_flipbook_renderer();
                        palette.open = false;
                    }
                    InspectorAction::SetModuleChoice {
                        module,
                        input,
                        choice,
                    } => set_module_choice(
                        &mut session,
                        &registry.0,
                        module,
                        input,
                        choice,
                        &localizer,
                    ),
                    InspectorAction::MoveModule(id, direction) => {
                        session.move_module(id, direction);
                    }
                    InspectorAction::DuplicateModule(id) => session.duplicate_module(id),
                    InspectorAction::DeleteModule(id) => {
                        if preview_module_deletion(&mut session, id) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                            workspace.clear();
                        }
                    }
                    InspectorAction::SetRendererMaterial(id, index) => {
                        if let Some(material) = session
                            .effect
                            .materials
                            .get(index)
                            .map(|material| material.id)
                        {
                            session.set_renderer_material(id, material);
                        }
                    }
                    InspectorAction::SetRendererBlend(id, blend) => {
                        session.set_renderer_blend(id, blend);
                    }
                    InspectorAction::SetRendererTexture(id, index) => {
                        let texture = index
                            .and_then(|index| session.effect.assets.get(index))
                            .filter(|asset| asset.kind == aestra_bevy::AssetKind::Texture)
                            .map(|asset| asset.id);
                        session.set_renderer_texture(id, texture);
                    }
                    InspectorAction::SetRendererFlipbook(id, index) => {
                        if let Some(flipbook) = session
                            .effect
                            .flipbooks
                            .get(index)
                            .map(|flipbook| flipbook.id)
                        {
                            session.set_renderer_flipbook(id, flipbook);
                        }
                    }
                    InspectorAction::SetFlipbookTimeSource(id, value) => {
                        session.set_flipbook_time_source(id, value);
                    }
                    InspectorAction::SetFlipbookPlayback(id, value) => {
                        session.set_flipbook_playback(id, value);
                    }
                    InspectorAction::AddEventLink { trigger, target } => {
                        let target_name = session
                            .effect
                            .emitters
                            .iter()
                            .find(|emitter| emitter.id == target)
                            .map(|emitter| emitter.name.clone());
                        let result = session.add_event_link(trigger, target);
                        let status = match result {
                            Ok(_) => InspectorStatus::EventAdded {
                                trigger: localized_event_trigger(&localizer, trigger),
                                target: target_name.unwrap_or_else(|| target.to_string()),
                            },
                            Err(crate::session::EventLinkError::SameEmitter) => {
                                InspectorStatus::EventSelfTarget
                            }
                            Err(crate::session::EventLinkError::Duplicate) => {
                                InspectorStatus::EventDuplicate
                            }
                            Err(crate::session::EventLinkError::TargetMissing) => {
                                InspectorStatus::EventTargetMissing
                            }
                        };
                        set_inspector_status(&mut session, &localizer, status);
                    }
                    InspectorAction::DeleteEventLink(id) => {
                        if session.remove_event_link(id) {
                            set_inspector_status(
                                &mut session,
                                &localizer,
                                InspectorStatus::EventRemoved,
                            );
                        } else {
                            set_inspector_status(
                                &mut session,
                                &localizer,
                                InspectorStatus::TargetUnavailable,
                            );
                        }
                    }
                    InspectorAction::RepairEffectClipSource { clip, source } => {
                        let result = catalog
                            .as_deref()
                            .ok_or_else(|| "the project effect catalog is unavailable".to_owned())
                            .and_then(|catalog| {
                                let clip = session
                                    .effect
                                    .effect_clips
                                    .iter()
                                    .find(|candidate| candidate.id == clip)
                                    .cloned()
                                    .ok_or_else(|| {
                                        "the effect clip is no longer available".to_owned()
                                    })?;
                                effect_clip_repair_source(catalog, &session.effect, &clip, source)
                                    .map(|_| ())
                            });
                        match result {
                            Ok(()) => {
                                session.execute(
                                    localizer.text("inspector-repair-effect-clip-command"),
                                    EffectCommand::SetEffectClipSource { id: clip, source },
                                    true,
                                );
                                if let Some(repair) = repair.as_deref_mut() {
                                    repair.query.clear();
                                }
                            }
                            Err(reason) => set_inspector_status(
                                &mut session,
                                &localizer,
                                InspectorStatus::RepairRejected(reason),
                            ),
                        }
                    }
                    InspectorAction::DuplicateRenderer(id) => session.duplicate_renderer(id),
                    InspectorAction::DeleteRenderer(id) => {
                        if preview_renderer_deletion(&mut session, id) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                            workspace.clear();
                        }
                    }
                    InspectorAction::ToggleSection(section) => {
                        if toggle_persisted_inspector_section(&session, &mut settings, section) {
                            persist_editor_settings(
                                &settings,
                                &mut settings_persistence,
                                &mut session,
                                &localizer,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn preview_module_deletion(session: &mut EditorSession, module: ModuleId) -> bool {
    let emitter = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        "Delete module",
        EffectCommand::RemoveModule { emitter, module },
    ))
}

fn preview_renderer_deletion(session: &mut EditorSession, renderer: RendererId) -> bool {
    let emitter = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        "Delete renderer",
        EffectCommand::RemoveRenderer { emitter, renderer },
    ))
}

// Inspector domain implementation.
fn semantic_target_exists(effect: &EffectAsset, target: SemanticTarget) -> bool {
    match target {
        SemanticTarget::Effect(id) => effect.id == id,
        SemanticTarget::EffectClip(id) => effect.effect_clips.iter().any(|clip| clip.id == id),
        SemanticTarget::Parameter(id) => effect.parameters.iter().any(|value| value.id == id),
        SemanticTarget::Emitter(id) => effect.emitters.iter().any(|emitter| emitter.id == id),
        SemanticTarget::Module(id) => effect
            .emitters
            .iter()
            .flat_map(|emitter| emitter.modules.iter())
            .any(|module| module.id == id),
        SemanticTarget::Renderer(id) => effect
            .emitters
            .iter()
            .flat_map(|emitter| emitter.renderers.iter())
            .any(|renderer| renderer.id == id),
        SemanticTarget::Event(id) => effect.events.iter().any(|event| event.id == id),
        SemanticTarget::Curve(_) | SemanticTarget::Gradient(_) => false,
    }
}

pub(crate) fn focus_compiled_target(
    session: &mut EditorSession,
    focus: &mut InspectorFocus,
    target: SemanticTarget,
    localizer: &Localizer,
) -> bool {
    if !semantic_target_exists(&session.effect, target) {
        return false;
    }
    if matches!(
        target,
        SemanticTarget::Emitter(_) | SemanticTarget::Module(_) | SemanticTarget::Renderer(_)
    ) {
        session.selection.primary = target;
    }
    focus.target = Some(target);
    focus.wait_frames = 2;
    focus.highlight = Some(target);
    focus.highlight_remaining = INSPECTOR_HIGHLIGHT_DURATION;
    set_inspector_status(
        session,
        localizer,
        InspectorStatus::SelectedCompiled(target.to_string()),
    );
    session.ui_revision += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EFFECT_PATH, EFFECT_SOURCE};
    use aestra_bevy::EffectClipSeed;

    fn test_localizer() -> Localizer {
        Localizer::new("en-US").unwrap()
    }

    #[test]
    fn repairing_a_clip_source_preserves_instance_state_and_is_undoable() {
        let temporary = tempfile::tempdir().unwrap();
        let mut replacement = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        replacement.id = aestra_bevy::EffectId::from_u128(0xc41d);
        replacement.name = "Replacement".into();
        replacement.duration = 4.0;
        replacement.looping = false;
        replacement.effect_clips.clear();
        replacement
            .save_ron(temporary.path().join("replacement.aestra.ron"))
            .unwrap();

        let missing = EffectAssetRef::new(aestra_bevy::EffectId::from_u128(0xdead));
        let replacement_ref = EffectAssetRef::new(replacement.id);
        let mut owner = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        owner.id = aestra_bevy::EffectId::from_u128(0xa11ce);
        owner.effect_clips.clear();
        let mut clip = EffectClip::new(missing, 0.75, 1.5);
        clip.source_offset = 0.5;
        clip.transform.translation = [2.0, -1.0, 3.5];
        clip.seed = EffectClipSeed::Fixed(77);
        let clip_id = clip.id;
        owner.effect_clips.push(clip.clone());

        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.effect = owner;
        let original = session.effect.effect_clips[0].clone();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        assert!(
            catalog
                .effect_clip_dependency_error(&session.effect, clip_id)
                .is_some()
        );

        let source = effect_clip_repair_source(
            &catalog,
            &session.effect,
            &session.effect.effect_clips[0],
            replacement_ref,
        )
        .unwrap();
        assert_eq!(source.id, replacement.id);
        assert!(session.execute(
            "Repair effect clip reference",
            EffectCommand::SetEffectClipSource {
                id: clip_id,
                source: replacement_ref,
            },
            true,
        ));

        let repaired = &session.effect.effect_clips[0];
        assert_eq!(repaired.source, replacement_ref);
        assert_eq!(repaired.id, original.id);
        assert_eq!(repaired.start_time, original.start_time);
        assert_eq!(repaired.source_offset, original.source_offset);
        assert_eq!(repaired.duration, original.duration);
        assert_eq!(repaired.transform, original.transform);
        assert_eq!(repaired.seed, original.seed);

        session.undo();
        assert_eq!(session.effect.effect_clips[0], original);
        session.redo();
        assert_eq!(session.effect.effect_clips[0].source, replacement_ref);
    }

    #[test]
    fn repair_candidates_reject_invalid_windows_and_reference_cycles() {
        let temporary = tempfile::tempdir().unwrap();
        let mut owner = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        owner.id = aestra_bevy::EffectId::from_u128(0xa11ce);
        owner.effect_clips.clear();
        let missing = EffectAssetRef::new(aestra_bevy::EffectId::from_u128(0xdead));
        let mut clip = EffectClip::new(missing, 0.0, 2.5);
        clip.source_offset = 0.75;
        owner.effect_clips.push(clip.clone());

        let mut short = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        short.id = aestra_bevy::EffectId::from_u128(0x5107);
        short.name = "Short".into();
        short.looping = false;
        short.effect_clips.clear();
        short
            .save_ron(temporary.path().join("short.aestra.ron"))
            .unwrap();

        let mut cycle = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        cycle.id = aestra_bevy::EffectId::from_u128(0xc1c1e);
        cycle.name = "Cycle".into();
        cycle.effect_clips = vec![EffectClip::new(EffectAssetRef::new(owner.id), 0.0, 0.5)];
        cycle
            .save_ron(temporary.path().join("cycle.aestra.ron"))
            .unwrap();
        owner
            .save_ron(temporary.path().join("owner.aestra.ron"))
            .unwrap();

        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let timing_error =
            effect_clip_repair_source(&catalog, &owner, &clip, EffectAssetRef::new(short.id))
                .unwrap_err();
        assert!(timing_error.contains("beyond the source duration"));
        assert!(
            effect_clip_repair_source(&catalog, &owner, &clip, EffectAssetRef::new(cycle.id))
                .is_err()
        );
    }

    #[test]
    fn effect_and_emitter_names_are_editable_semantic_fields() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let original_effect_name = session.effect.name.clone();
        let original_emitter_name = session.selected_layer().name.clone();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(test_localizer())
            .add_observer(handle_document_text_change);
        let effect_name = app.world_mut().spawn(DocumentTextControl::EffectName).id();
        let emitter_name = app.world_mut().spawn(DocumentTextControl::EmitterName).id();

        app.world_mut().trigger(ValueChange {
            source: effect_name,
            value: "Renamed Effect".to_owned(),
            is_final: true,
        });
        app.world_mut().trigger(ValueChange {
            source: emitter_name,
            value: "Renamed Emitter".to_owned(),
            is_final: true,
        });
        app.update();

        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert_eq!(session.effect.name, "Renamed Effect");
        assert_eq!(session.selected_layer().name, "Renamed Emitter");
        assert!(session.dirty);
        assert!(session.can_undo());
        session.undo();
        session.undo();
        assert_eq!(session.effect.name, original_effect_name);
        assert_eq!(session.selected_layer().name, original_emitter_name);
    }

    #[test]
    fn inspector_action_activation_uses_the_feathers_contract() {
        let mut app = App::new();
        app.add_observer(queue_inspector_action_activation);
        let action = app
            .world_mut()
            .spawn((
                InspectorAction::CloseModulePalette,
                FeathersActionButton,
                Interaction::None,
            ))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert_eq!(action.get::<Interaction>(), Some(&Interaction::Pressed));
    }

    #[test]
    fn inspector_actions_are_executed_by_the_inspector_plugin_path() {
        let temporary = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.insert_resource(EditorSession::from_embedded_sample(
            EFFECT_SOURCE,
            EFFECT_PATH,
        ))
        .insert_resource(MenuState::default())
        .insert_resource(EditorModuleRegistry::default())
        .insert_resource(ModulePaletteState::default())
        .insert_resource(CurvesState::default())
        .insert_resource(WorkspaceLayout::default())
        .insert_resource(EditorSettings::default())
        .insert_resource(SettingsPersistence::for_test(
            temporary.path().join("settings.ron"),
        ))
        .insert_resource(test_localizer())
        .add_systems(Update, handle_inspector_actions);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            InspectorAction::OpenModulePalette(StackStage::ParticleSpawn),
            BackgroundColor(theme::BUTTON),
        ));

        app.update();

        let palette = app.world().resource::<ModulePaletteState>();
        assert!(palette.open);
        assert_eq!(palette.stage, StackStage::ParticleSpawn);
    }

    #[test]
    fn inspector_disclosure_persists_without_requesting_a_ui_rebuild() {
        let temporary = tempfile::tempdir().unwrap();
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session.selected_layer().modules[0].id;
        let revision = session.ui_revision;
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(MenuState::default())
            .insert_resource(EditorModuleRegistry::default())
            .insert_resource(ModulePaletteState::default())
            .insert_resource(CurvesState::default())
            .insert_resource(WorkspaceLayout::default())
            .insert_resource(EditorSettings::default())
            .insert_resource(SettingsPersistence::for_test(
                temporary.path().join("settings.ron"),
            ))
            .insert_resource(test_localizer())
            .add_systems(Update, handle_inspector_actions);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            InspectorAction::ToggleSection(InspectorSection::Module(module)),
            BackgroundColor(theme::BUTTON),
        ));

        app.update();

        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            revision,
            "a disclosure must update its existing card instead of rebuilding the editor"
        );
    }

    #[test]
    fn inspector_event_action_creates_an_undoable_semantic_link() {
        let temporary = tempfile::tempdir().unwrap();
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.new_effect();
        let target = session.selected_layer().id;
        session.add_layer();
        let source = session.selected_layer().id;
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(MenuState::default())
            .insert_resource(EditorModuleRegistry::default())
            .insert_resource(ModulePaletteState::default())
            .insert_resource(CurvesState::default())
            .insert_resource(WorkspaceLayout::default())
            .insert_resource(EditorSettings::default())
            .insert_resource(SettingsPersistence::for_test(
                temporary.path().join("settings.ron"),
            ))
            .insert_resource(test_localizer())
            .add_systems(Update, handle_inspector_actions);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            InspectorAction::AddEventLink {
                trigger: EventTrigger::OnDeath,
                target,
            },
            BackgroundColor(theme::BUTTON),
        ));

        app.update();

        let session = app.world_mut().resource_mut::<EditorSession>();
        assert_eq!(session.effect.events.len(), 1);
        assert_eq!(session.effect.events[0].source, source);
        assert!(session.can_undo());
        assert!(session.status.contains("On death"));
    }

    #[test]
    fn inspector_outcomes_are_localized_and_preserve_semantic_details() {
        let english = test_localizer();
        assert_eq!(
            localize_inspector_status(InspectorStatus::TargetUnavailable, &english),
            "Inspector target is no longer available"
        );
        let finite = localize_inspector_status(
            InspectorStatus::FiniteNumberRequired("spawn_rate".into()),
            &english,
        );
        assert!(finite.contains("spawn_rate"));
        assert!(finite.ends_with(" requires a finite number"));

        let french = Localizer::new("fr-FR").unwrap();
        assert_eq!(
            localize_inspector_status(InspectorStatus::ChoiceUnavailable, &french),
            "Le choix n’est plus disponible"
        );
        let selected =
            localize_inspector_status(InspectorStatus::Selected("module/shape".into()), &french);
        assert!(selected.contains("module/shape"));
    }

    #[test]
    fn module_deletion_preview_remains_one_undoable_transaction() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let source = session.selected_layer().modules[0].id;
        session.duplicate_module(source);
        let SemanticTarget::Module(module) = session.selection.primary else {
            panic!("duplicating a module should select the duplicate");
        };
        let original_count = session.selected_layer().modules.len();

        assert!(preview_module_deletion(&mut session, module));
        assert_eq!(session.selected_layer().modules.len(), original_count);
        assert!(session.pending_change.is_some());
        session.apply_pending_change();
        assert_eq!(session.selected_layer().modules.len(), original_count - 1);
        session.undo();
        assert_eq!(session.selected_layer().modules.len(), original_count);
    }

    // Inspector domain tests.
    #[test]
    fn compiled_navigation_focuses_the_exact_inspector_target() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let target = SemanticTarget::Module(session.effect.emitters[3].modules[2].id);
        let mut focus = InspectorFocus::default();

        assert!(focus_compiled_target(
            &mut session,
            &mut focus,
            target,
            &test_localizer(),
        ));
        assert_eq!(session.selection.primary, target);
        assert_eq!(focus.target, Some(target));
        assert_eq!(focus.wait_frames, 2);
        assert_eq!(focus.highlight, Some(target));
        assert_eq!(focus.highlight_remaining, INSPECTOR_HIGHLIGHT_DURATION);
        assert_eq!(session.selected_layer_index(), 3);
    }

    #[test]
    fn palette_search_uses_registry_names_categories_ids_and_tags() {
        let registry = ModuleRegistry::builtin();
        let motion = registry
            .iter()
            .find(|metadata| metadata.type_id.0.ends_with("motion"))
            .unwrap();
        assert!(module_matches(motion, "motion"));
        assert!(module_matches(motion, "forces"));
        assert!(module_matches(motion, "force"));
        assert!(!module_matches(motion, "color"));
    }

    #[test]
    fn inspector_input_localization_uses_fluent_and_preserves_custom_metadata() {
        let localizer = Localizer::new("fr-FR").unwrap();
        assert_eq!(
            localized_inspector_input(&localizer, "spawn_rate", "Spawn Rate", false),
            "Taux d’émission"
        );
        assert_eq!(
            localized_inspector_input(&localizer, "custom_gain", "Custom Gain", false),
            "Custom Gain"
        );
    }

    #[test]
    fn inspector_number_rejects_non_finite_values() {
        assert_eq!(clamp_inspector_number(f32::INFINITY, None, None), None);
        assert_eq!(
            clamp_inspector_number(-5.0, Some(0.0), Some(10.0)),
            Some(0.0)
        );
    }

    #[test]
    fn inspector_typing_does_not_rebuild_or_commit_until_final() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let original = inspector_module_parameter(&session, module, "spawn_rate").unwrap();
        let revision = session.ui_revision;
        let mut app = App::new();
        app.insert_resource(session);
        app.insert_resource(test_localizer());
        app.add_observer(handle_inspector_scalar_change);
        let control = app
            .world_mut()
            .spawn(InspectorNumberControl {
                module,
                parameter: "spawn_rate",
                component: 0,
                kind: InspectorNumberKind::Scalar,
                step: 5.0,
                min: Some(0.0),
                max: None,
            })
            .id();

        app.world_mut().trigger(ValueChange {
            source: control,
            value: 123.0_f32,
            is_final: false,
        });
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(
            inspector_module_parameter(session, module, "spawn_rate"),
            Some(original)
        );
        assert_eq!(session.ui_revision, revision);
    }

    #[test]
    fn inspector_range_edit_preserves_ordering() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "lifetime").is_some())
            .unwrap()
            .id;
        let control = InspectorNumberControl {
            module,
            parameter: "lifetime",
            component: 0,
            kind: InspectorNumberKind::Range,
            step: 0.1,
            min: Some(0.05),
            max: None,
        };

        assert!(apply_inspector_number(
            &mut session,
            control,
            99.0,
            &test_localizer(),
        ));
        let Value::Range(range) = inspector_module_parameter(&session, module, "lifetime").unwrap()
        else {
            panic!("lifetime should remain a range");
        };
        assert_eq!(range.min, range.max);
    }

    #[test]
    fn inspector_scrub_previews_live_and_commits_one_undoable_edit() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let original = inspector_module_parameter(&session, module, "spawn_rate").unwrap();
        let target = NumericScrubTarget::Inspector(InspectorNumberControl {
            module,
            parameter: "spawn_rate",
            component: 0,
            kind: InspectorNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: None,
        });

        assert!(preview_numeric_scrub(&mut session, target, 29.0));
        assert_eq!(
            inspector_module_parameter(&session, module, "spawn_rate"),
            Some(original.clone()),
            "drag preview must not mutate the document"
        );
        commit_numeric_scrub(&mut session, target, 29.0, &test_localizer());
        assert_eq!(
            inspector_module_parameter(&session, module, "spawn_rate"),
            Some(Value::Scalar(29.0))
        );
        session.undo();
        assert_eq!(
            inspector_module_parameter(&session, module, "spawn_rate"),
            Some(original)
        );
    }

    #[test]
    fn bounded_slider_commit_preserves_the_inspector_tree_and_is_undoable() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spread_degrees").is_some())
            .unwrap()
            .id;
        let original = inspector_module_parameter(&session, module, "spread_degrees").unwrap();
        let target = NumericScrubTarget::Inspector(InspectorNumberControl {
            module,
            parameter: "spread_degrees",
            component: 0,
            kind: InspectorNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: Some(360.0),
        });
        let ui_revision = session.ui_revision;

        assert!(preview_numeric_scrub(&mut session, target, 75.0));
        assert!(commit_bounded_slider(&mut session, target, 75.0));
        assert_eq!(session.ui_revision, ui_revision);
        assert_eq!(
            inspector_module_parameter(&session, module, "spread_degrees"),
            Some(Value::Scalar(75.0))
        );

        session.undo();
        assert_eq!(
            inspector_module_parameter(&session, module, "spread_degrees"),
            Some(original)
        );
    }

    #[test]
    fn inspector_scrub_uses_metadata_steps_and_modifier_precision() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let control = InspectorNumberControl {
            module,
            parameter: "spawn_rate",
            component: 0,
            kind: InspectorNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: None,
        };
        let target = NumericScrubTarget::Inspector(control);
        assert_eq!(numeric_scrub_step(target), 5.0);
        assert_eq!(numeric_scrub_delta(8.0, 5.0, 1.0), 5.0);
        assert_eq!(
            numeric_scrub_delta(8.0, 5.0, numeric_scrub_multiplier(true, false)),
            0.5
        );
        assert_eq!(numeric_scrub_multiplier(false, true), 10.0);
        assert_eq!(normalize_numeric_scrub_value(&session, target, -100.0), 0.0);
        assert_eq!(numeric_scrub_precision(target, 1.0), 0);
        assert_eq!(numeric_scrub_precision(target, 0.1), 1);
        assert_eq!(numeric_scrub_precision(target, 10.0), 0);
        assert_eq!(format_numeric_scrub_value(target, 22.499, 1.0), "22");
        assert_eq!(format_numeric_scrub_value(target, 22.499, 0.1), "22.5");

        let translation = NumericScrubTarget::Emitter(EmitterNumberControl::Translation(0));
        assert_eq!(numeric_scrub_precision(translation, 1.0), 1);
        assert_eq!(numeric_scrub_precision(translation, 0.1), 2);
        assert_eq!(numeric_scrub_precision(translation, 10.0), 0);
        assert_eq!(
            format_numeric_scrub_value(translation, 13.86246, 1.0),
            "13.9"
        );
        assert_eq!(
            format_numeric_scrub_value(translation, 13.86246, 0.1),
            "13.86"
        );
        assert_eq!(
            format_numeric_scrub_value(translation, 13.86246, 10.0),
            "14"
        );
    }

    #[test]
    fn inspector_sections_use_compact_defaults_and_persist_type_preferences() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let mut settings = EditorSettings::default();
        let emission = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.stage == StageKind::EmitterUpdate)
            .unwrap();
        let motion = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.stage == StageKind::ParticleUpdate)
            .unwrap();
        let renderer = session.selected_layer().renderers.first().unwrap();

        assert!(!inspector_module_collapsed(&settings, emission));
        assert!(inspector_module_collapsed(&settings, motion));
        assert!(inspector_renderer_collapsed(&settings, renderer));

        assert!(toggle_persisted_inspector_section(
            &session,
            &mut settings,
            InspectorSection::Module(motion.id),
        ));
        assert!(!inspector_module_collapsed(&settings, motion));
        assert_eq!(
            settings
                .inspector
                .section_expansion
                .get(&inspector_module_key(motion)),
            Some(&true)
        );

        assert!(toggle_persisted_inspector_section(
            &session,
            &mut settings,
            InspectorSection::Renderer(renderer.id),
        ));
        assert!(!inspector_renderer_collapsed(&settings, renderer));
        assert_eq!(
            settings
                .inspector
                .section_expansion
                .get(&inspector_renderer_key(renderer)),
            Some(&true)
        );
    }

    #[test]
    fn inspector_number_edit_is_clamped_semantic_and_undoable() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let original = inspector_module_parameter(&session, module, "spawn_rate").unwrap();
        let control = InspectorNumberControl {
            module,
            parameter: "spawn_rate",
            component: 0,
            kind: InspectorNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: Some(30.0),
        };

        assert!(apply_inspector_number(
            &mut session,
            control,
            300.0,
            &test_localizer(),
        ));
        assert_eq!(
            inspector_module_parameter(&session, module, "spawn_rate"),
            Some(Value::Scalar(30.0))
        );
        assert!(session.can_undo());

        session.undo();
        assert_eq!(
            inspector_module_parameter(&session, module, "spawn_rate"),
            Some(original)
        );
    }

    #[test]
    fn inspector_edits_volumetric_shape_dimensions_semantically() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "shape").is_some())
            .unwrap()
            .id;
        let registry = ModuleRegistry::builtin();
        set_module_choice(&mut session, &registry, module, 0, 5, &test_localizer());

        let control = InspectorNumberControl {
            module,
            parameter: "shape",
            component: 2,
            kind: InspectorNumberKind::Shape,
            step: 0.1,
            min: Some(0.1),
            max: None,
        };
        assert!(apply_inspector_number(
            &mut session,
            control,
            18.0,
            &test_localizer(),
        ));
        assert_eq!(
            inspector_module_parameter(&session, module, "shape"),
            Some(Value::Shape(EmitterShape::Box {
                half_extents: [12.0, 12.0, 18.0],
            }))
        );
    }

    #[test]
    fn inspector_choice_selects_the_requested_shape_directly() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "shape").is_some())
            .unwrap()
            .id;
        let registry = ModuleRegistry::builtin();

        set_module_choice(&mut session, &registry, module, 0, 7, &test_localizer());

        assert_eq!(
            inspector_module_parameter(&session, module, "shape"),
            Some(Value::Shape(EmitterShape::Cone {
                radius: 12.0,
                depth: 24.0,
            }))
        );
    }

    #[test]
    fn inspector_emitter_transform_components_are_semantic_and_undoable() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        assert!(set_emitter_transform_component(
            &mut session,
            EmitterNumberControl::Translation(2),
            12.5,
            false,
        ));
        assert_eq!(session.selected_layer().transform.translation[2], 12.5);
        session.undo();
        assert_eq!(
            session.selected_layer().transform,
            EmitterTransform::default()
        );
    }

    #[test]
    fn inspector_effect_clip_transform_is_semantic_and_undoable() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let clip = aestra_bevy::EffectClip::new(aestra_bevy::EffectId::from_u128(0xC11D), 0.0, 1.0);
        let clip_id = clip.id;
        session.effect.effect_clips.push(clip);
        let control = EffectClipNumberControl {
            clip: clip_id,
            control: EmitterNumberControl::Translation(0),
        };
        let command = effect_clip_transform_command(&session, control, 8.5).unwrap();

        assert!(session.execute("Transformed effect clip", command, false));
        assert_eq!(
            session
                .effect
                .effect_clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .unwrap()
                .transform
                .translation[0],
            8.5
        );
        session.undo();
        assert_eq!(
            session
                .effect
                .effect_clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .unwrap()
                .transform,
            EmitterTransform::default()
        );
    }
}
fn scroll_inspector_to_focus(
    mut commands: Commands,
    mut focus: ResMut<InspectorFocus>,
    targets: Query<(Entity, &InspectorSemanticTarget)>,
) {
    let Some(target) = focus.target else {
        return;
    };
    if focus.wait_frames > 0 {
        focus.wait_frames -= 1;
        return;
    }
    if let Some((entity, _)) = targets
        .iter()
        .find(|(_, candidate)| candidate.target == target)
    {
        commands.trigger(ScrollIntoView { entity });
    }
    focus.target = None;
}

fn update_inspector_highlight(
    time: Res<Time>,
    mut focus: ResMut<InspectorFocus>,
    mut targets: Query<(&InspectorSemanticTarget, &mut BorderColor)>,
) {
    let Some(highlight) = focus.highlight else {
        return;
    };
    focus.highlight_remaining = (focus.highlight_remaining - time.delta_secs()).max(0.0);
    let strength = (focus.highlight_remaining / INSPECTOR_HIGHLIGHT_DURATION)
        .clamp(0.0, 1.0)
        .powi(2);
    for (target, mut border) in &mut targets {
        if target.target == highlight {
            *border = BorderColor::all(target.base_border.mix(&theme::ACCENT, strength));
        }
    }
    if focus.highlight_remaining == 0.0 {
        focus.highlight = None;
    }
}

pub(crate) fn set_module_choice(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module_id: ModuleId,
    input_index: u8,
    choice: u8,
    localizer: &Localizer,
) {
    let Some(module) = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == module_id)
    else {
        set_inspector_status(session, localizer, InspectorStatus::ModuleMissing);
        return;
    };
    let Some(input) = registry
        .get(&module.module_type)
        .and_then(|metadata| metadata.inputs.get(input_index as usize))
    else {
        set_inspector_status(
            session,
            localizer,
            InspectorStatus::InputMetadataUnavailable,
        );
        return;
    };
    if !matches!(input.control, InputControl::Choice) {
        set_inspector_status(
            session,
            localizer,
            InspectorStatus::NotChoice(input.display_name.into()),
        );
        return;
    }
    let current = module_parameter(module, input.name);
    let shape = match choice {
        0 => EmitterShape::Point,
        1 => match current {
            Some(Value::Shape(EmitterShape::Circle { radius })) => EmitterShape::Circle { radius },
            _ => EmitterShape::Circle { radius: 12.0 },
        },
        2 => match current {
            Some(Value::Shape(EmitterShape::Ring { radius })) => EmitterShape::Ring { radius },
            _ => EmitterShape::Ring { radius: 12.0 },
        },
        3 => match current {
            Some(Value::Shape(EmitterShape::Sphere { radius })) => EmitterShape::Sphere { radius },
            _ => EmitterShape::Sphere { radius: 12.0 },
        },
        4 => match current {
            Some(Value::Shape(EmitterShape::Hemisphere { radius })) => {
                EmitterShape::Hemisphere { radius }
            }
            _ => EmitterShape::Hemisphere { radius: 12.0 },
        },
        5 => match current {
            Some(Value::Shape(EmitterShape::Box { half_extents })) => {
                EmitterShape::Box { half_extents }
            }
            _ => EmitterShape::Box {
                half_extents: [12.0; 3],
            },
        },
        6 => match current {
            Some(Value::Shape(EmitterShape::Cylinder { radius, depth })) => {
                EmitterShape::Cylinder { radius, depth }
            }
            _ => EmitterShape::Cylinder {
                radius: 12.0,
                depth: 24.0,
            },
        },
        7 => match current {
            Some(Value::Shape(EmitterShape::Cone { radius, depth })) => {
                EmitterShape::Cone { radius, depth }
            }
            _ => EmitterShape::Cone {
                radius: 12.0,
                depth: 24.0,
            },
        },
        _ => {
            set_inspector_status(session, localizer, InspectorStatus::ChoiceUnavailable);
            return;
        }
    };
    session.set_module_parameter(module_id, input.name, Value::Shape(shape));
}

fn module_palette_keyboard(
    mut input: MessageReader<KeyboardInput>,
    mut palette: ResMut<ModulePaletteState>,
    mut session: ResMut<EditorSession>,
) {
    if !palette.open {
        return;
    }
    let mut changed = false;
    for event in input.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        match event.key_code {
            KeyCode::Escape => {
                palette.open = false;
                changed = true;
            }
            KeyCode::Backspace => {
                changed |= palette.query.pop().is_some();
            }
            _ => {
                if let Some(text) = &event.text {
                    let clean = text.chars().filter(|character| !character.is_control());
                    let previous = palette.query.len();
                    palette.query.extend(clean);
                    changed |= palette.query.len() != previous;
                }
            }
        }
    }
    if changed {
        session.ui_revision += 1;
    }
}

fn sync_emitter_capacity_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<Entity, Added<EmitterCapacityControl>>,
) {
    let value = session.selected_layer().max_particles.min(i32::MAX as u32) as i32;
    for entity in &controls {
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::I32(value),
        });
    }
}

fn sync_emitter_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    gizmo: Res<TransformGizmoState>,
    interaction: Res<EmitterTransformGizmoInteraction>,
    proxy: Query<Ref<Transform>, With<EmitterTransformGizmoProxy>>,
    controls: Query<(Entity, Ref<EmitterNumberControl>)>,
) {
    let live_transform = proxy
        .single()
        .ok()
        .filter(|transform| (gizmo.active || interaction.is_active()) && transform.is_changed())
        .map(|transform| emitter_transform_from_bevy(&transform));
    for (entity, control) in &controls {
        if !control.is_added() && live_transform.is_none() {
            continue;
        }
        let value = live_transform
            .and_then(|transform| emitter_transform_component_value(transform, *control))
            .unwrap_or_else(|| emitter_number_input_value(&session, *control));
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::F32(value),
        });
    }
}

fn sync_effect_clip_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    gizmo: Res<TransformGizmoState>,
    interaction: Res<EmitterTransformGizmoInteraction>,
    proxy: Query<Ref<Transform>, With<EmitterTransformGizmoProxy>>,
    controls: Query<(Entity, Ref<EffectClipNumberControl>)>,
) {
    let live_transform = proxy
        .single()
        .ok()
        .filter(|transform| (gizmo.active || interaction.is_active()) && transform.is_changed())
        .map(|transform| emitter_transform_from_bevy(&transform));
    for (entity, control) in &controls {
        if !control.is_added() && live_transform.is_none() {
            continue;
        }
        let value = live_transform
            .and_then(|transform| emitter_transform_component_value(transform, control.control))
            .or_else(|| effect_clip_number_input_value(&session, *control));
        if let Some(value) = value {
            commands.trigger(UpdateNumberInput {
                entity,
                value: NumberInputValue::F32(value),
            });
        }
    }
}

fn emitter_number_input_value(session: &EditorSession, control: EmitterNumberControl) -> f32 {
    let transform = session.selected_layer().transform;
    match control {
        EmitterNumberControl::Start => session.selected_layer().start_time,
        EmitterNumberControl::Duration => session.selected_layer().duration,
        _ => emitter_transform_component_value(transform, control)
            .expect("transform control must resolve against an emitter transform"),
    }
}

fn emitter_transform_component_value(
    transform: EmitterTransform,
    control: EmitterNumberControl,
) -> Option<f32> {
    match control {
        EmitterNumberControl::Translation(component) => {
            Some(transform.translation[component as usize])
        }
        EmitterNumberControl::Rotation(component) => {
            let (x, y, z) = Quat::from_array(transform.rotation)
                .normalize()
                .to_euler(EulerRot::XYZ);
            Some([x.to_degrees(), y.to_degrees(), z.to_degrees()][component as usize])
        }
        EmitterNumberControl::Scale(component) => Some(transform.scale[component as usize]),
        EmitterNumberControl::Start | EmitterNumberControl::Duration => None,
    }
}

fn effect_clip_number_input_value(
    session: &EditorSession,
    control: EffectClipNumberControl,
) -> Option<f32> {
    let clip = session
        .effect
        .effect_clips
        .iter()
        .find(|clip| clip.id == control.clip)?;
    emitter_transform_component_value(clip.transform, control.control)
}

fn effect_clip_transform_command(
    session: &EditorSession,
    control: EffectClipNumberControl,
    value: f32,
) -> Option<EffectCommand> {
    let clip = session
        .effect
        .effect_clips
        .iter()
        .find(|clip| clip.id == control.clip)?;
    let mut transform = clip.transform;
    set_emitter_transform_value(&mut transform, control.control, value)?;
    Some(EffectCommand::SetEffectClipTransform {
        id: control.clip,
        transform,
    })
}

fn set_emitter_transform_component(
    session: &mut EditorSession,
    control: EmitterNumberControl,
    value: f32,
    rebuild_ui: bool,
) -> bool {
    if !value.is_finite() {
        return false;
    }
    let mut transform = session.selected_layer().transform;
    if set_emitter_transform_value(&mut transform, control, value).is_none() {
        return false;
    }
    session.set_selected_emitter_transform(transform, rebuild_ui)
}

fn set_emitter_transform_value(
    transform: &mut EmitterTransform,
    control: EmitterNumberControl,
    value: f32,
) -> Option<()> {
    if !value.is_finite() {
        return None;
    }
    match control {
        EmitterNumberControl::Translation(component) => {
            transform.translation[component as usize] = value;
        }
        EmitterNumberControl::Rotation(component) => {
            let (x, y, z) = Quat::from_array(transform.rotation)
                .normalize()
                .to_euler(EulerRot::XYZ);
            let mut euler = [x, y, z];
            euler[component as usize] = value.to_radians();
            transform.rotation = Quat::from_euler(EulerRot::XYZ, euler[0], euler[1], euler[2])
                .normalize()
                .to_array();
        }
        EmitterNumberControl::Scale(component) => {
            transform.scale[component as usize] = value.max(0.001);
        }
        EmitterNumberControl::Start | EmitterNumberControl::Duration => return None,
    }
    Some(())
}

fn sync_inspector_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &InspectorNumberControl), Added<InspectorNumberControl>>,
) {
    for (entity, control) in &controls {
        let Some(value) = inspector_number_input_value(&session, *control) else {
            continue;
        };
        commands.trigger(UpdateNumberInput { entity, value });
    }
}

fn sync_inspector_slider_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &InspectorSliderControl), Added<InspectorSliderControl>>,
) {
    for (entity, control) in &controls {
        let Some(NumberInputValue::F32(value)) = inspector_number_input_value(&session, control.0)
        else {
            continue;
        };
        commands.entity(entity).insert(SliderValue(value));
    }
}

fn sync_renderer_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &RendererNumberControl), Added<RendererNumberControl>>,
) {
    for (entity, control) in &controls {
        let Some(value) = renderer_number_input_value(&session, *control) else {
            continue;
        };
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::F32(value),
        });
    }
}

fn sync_renderer_slider_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &RendererSliderControl), Added<RendererSliderControl>>,
) {
    for (entity, control) in &controls {
        let Some(value) = renderer_number_input_value(&session, control.0) else {
            continue;
        };
        commands.entity(entity).insert(SliderValue(value));
    }
}

fn renderer_number_input_value(
    session: &EditorSession,
    control: RendererNumberControl,
) -> Option<f32> {
    let renderer_id = match control {
        RendererNumberControl::Softness(renderer)
        | RendererNumberControl::Uv(renderer, _)
        | RendererNumberControl::FlipbookFrameRate(renderer) => renderer,
    };
    let renderer = session
        .selected_layer()
        .renderers
        .iter()
        .find(|renderer| renderer.id == renderer_id)?;
    match control {
        RendererNumberControl::Softness(_) => {
            let material = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)?;
            let MaterialProperties::Sprite { softness, .. } = &material.properties;
            match softness {
                MaterialInput::Constant(value) => Some(*value),
                MaterialInput::Parameter(_) => None,
            }
        }
        RendererNumberControl::Uv(_, component) => {
            let material = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)?;
            let MaterialProperties::Sprite { uv, .. } = &material.properties;
            match component {
                0 => Some(uv.min[0]),
                1 => Some(uv.min[1]),
                2 => Some(uv.max[0]),
                3 => Some(uv.max[1]),
                _ => None,
            }
        }
        RendererNumberControl::FlipbookFrameRate(_) => {
            let RendererProperties::Flipbook { flipbook, .. } = renderer.properties else {
                return None;
            };
            session
                .effect
                .flipbooks
                .iter()
                .find(|definition| definition.id == flipbook)
                .map(|definition| definition.frame_rate)
        }
    }
}

fn inspector_number_input_value(
    session: &EditorSession,
    control: InspectorNumberControl,
) -> Option<NumberInputValue> {
    let value = inspector_module_parameter(session, control.module, control.parameter)?;
    match (control.kind, value) {
        (InspectorNumberKind::U32, Value::U32(value)) => {
            Some(NumberInputValue::I32(value.min(i32::MAX as u32) as i32))
        }
        (InspectorNumberKind::Scalar, Value::Scalar(value)) => Some(NumberInputValue::F32(value)),
        (InspectorNumberKind::Vector, Value::Vec2(value)) => value
            .get(control.component as usize)
            .copied()
            .map(NumberInputValue::F32),
        (InspectorNumberKind::Vector, Value::Vec3(value)) => value
            .get(control.component as usize)
            .copied()
            .map(NumberInputValue::F32),
        (InspectorNumberKind::Vector, Value::Vec4(value)) => value
            .get(control.component as usize)
            .copied()
            .map(NumberInputValue::F32),
        (InspectorNumberKind::Range, Value::Range(value)) => {
            Some(NumberInputValue::F32(if control.component == 0 {
                value.min
            } else {
                value.max
            }))
        }
        (InspectorNumberKind::Shape, Value::Shape(shape)) => {
            shape_dimension(shape, control.component).map(NumberInputValue::F32)
        }
        _ => None,
    }
}

fn shape_dimension(shape: EmitterShape, component: u8) -> Option<f32> {
    match (shape, component) {
        (EmitterShape::Circle { radius }, 0)
        | (EmitterShape::Ring { radius }, 0)
        | (EmitterShape::Sphere { radius }, 0)
        | (EmitterShape::Hemisphere { radius }, 0)
        | (EmitterShape::Cylinder { radius, .. }, 0)
        | (EmitterShape::Cone { radius, .. }, 0) => Some(radius),
        (EmitterShape::Cylinder { depth, .. }, 1) | (EmitterShape::Cone { depth, .. }, 1) => {
            Some(depth)
        }
        (EmitterShape::Box { half_extents }, component @ 0..=2) => {
            Some(half_extents[component as usize])
        }
        _ => None,
    }
}

fn shape_with_dimension(shape: EmitterShape, component: u8, value: f32) -> Option<EmitterShape> {
    Some(match (shape, component) {
        (EmitterShape::Circle { .. }, 0) => EmitterShape::Circle { radius: value },
        (EmitterShape::Ring { .. }, 0) => EmitterShape::Ring { radius: value },
        (EmitterShape::Sphere { .. }, 0) => EmitterShape::Sphere { radius: value },
        (EmitterShape::Hemisphere { .. }, 0) => EmitterShape::Hemisphere { radius: value },
        (EmitterShape::Cylinder { depth, .. }, 0) => EmitterShape::Cylinder {
            radius: value,
            depth,
        },
        (EmitterShape::Cylinder { radius, .. }, 1) => EmitterShape::Cylinder {
            radius,
            depth: value,
        },
        (EmitterShape::Cone { depth, .. }, 0) => EmitterShape::Cone {
            radius: value,
            depth,
        },
        (EmitterShape::Cone { radius, .. }, 1) => EmitterShape::Cone {
            radius,
            depth: value,
        },
        (EmitterShape::Box { mut half_extents }, component @ 0..=2) => {
            half_extents[component as usize] = value;
            EmitterShape::Box { half_extents }
        }
        _ => return None,
    })
}

fn handle_document_text_change(
    change: On<ValueChange<String>>,
    controls: Query<&DocumentTextControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let value = change.value.trim();
    if value.is_empty() {
        let target = match control {
            DocumentTextControl::EffectName => localizer.text("inspector-effect"),
            DocumentTextControl::EmitterName => localizer.text("inspector-emitter"),
        };
        set_inspector_status(
            &mut session,
            &localizer,
            InspectorStatus::NameRequired(target),
        );
        session.ui_revision += 1;
        return;
    }
    let changed = match control {
        DocumentTextControl::EffectName => session.set_effect_name(value),
        DocumentTextControl::EmitterName => session.set_selected_emitter_name(value),
    };
    if changed {
        let target = match control {
            DocumentTextControl::EffectName => {
                localizer.text("inspector-effect-name-status-target")
            }
            DocumentTextControl::EmitterName => {
                localizer.text("inspector-emitter-name-status-target")
            }
        };
        set_inspector_status(&mut session, &localizer, InspectorStatus::Updated(target));
    }
}

fn handle_document_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&DocumentToggleControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let (changed, target) = match control {
        DocumentToggleControl::EmitterEnabled => (
            session.set_selected_emitter_enabled(change.value),
            localizer.text("inspector-emitter-enabled"),
        ),
    };
    if changed {
        set_inspector_status(&mut session, &localizer, InspectorStatus::Updated(target));
    }
}

fn handle_emitter_capacity_change(
    change: On<ValueChange<i32>>,
    controls: Query<(), With<EmitterCapacityControl>>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final || !controls.contains(change.source) {
        return;
    }
    let value = change.value.max(1) as u32;
    if session.set_selected_emitter_capacity(value) {
        set_inspector_status(
            &mut session,
            &localizer,
            InspectorStatus::Updated(localizer.text("inspector-emitter-capacity")),
        );
    }
}

fn handle_inspector_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&InspectorToggleControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let Some(current) = inspector_module_parameter(&session, control.module, control.parameter)
    else {
        set_inspector_status(&mut session, &localizer, InspectorStatus::TargetUnavailable);
        return;
    };
    let value = Value::Bool(change.value);
    if current != value {
        session.set_module_parameter(control.module, control.parameter, value);
    }
}

fn handle_module_enabled_change(
    change: On<ValueChange<bool>>,
    controls: Query<&ModuleEnabledControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let enabled = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == control.0)
        .map(|module| module.enabled);
    if enabled.is_some_and(|enabled| enabled != change.value) {
        session.toggle_module(control.0);
    }
}

fn handle_renderer_enabled_change(
    change: On<ValueChange<bool>>,
    controls: Query<&RendererEnabledControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let enabled = session
        .selected_layer()
        .renderers
        .iter()
        .find(|renderer| renderer.id == control.0)
        .map(|renderer| renderer.enabled);
    if enabled.is_some_and(|enabled| enabled != change.value) {
        session.toggle_renderer(control.0);
    }
}

fn handle_renderer_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&RendererNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if renderer_number_input_value(&session, *control)
        .is_some_and(|current| (change.value - current).abs() <= f32::EPSILON)
    {
        return;
    }
    match *control {
        RendererNumberControl::Softness(renderer) => {
            session.set_renderer_softness(renderer, change.value)
        }
        RendererNumberControl::Uv(renderer, component) => {
            session.set_renderer_uv(renderer, component, change.value)
        }
        RendererNumberControl::FlipbookFrameRate(renderer) => {
            session.set_flipbook_frame_rate(renderer, change.value)
        }
    }
}

fn handle_bounded_slider_change(
    change: On<ValueChange<f32>>,
    inspector_controls: Query<&InspectorSliderControl>,
    renderer_controls: Query<&RendererSliderControl>,
    pairs: Query<&SliderNumberInputPair>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<BoundedSliderState>,
) {
    if !change.value.is_finite() {
        return;
    }
    let target = if let Ok(control) = inspector_controls.get(change.source) {
        NumericScrubTarget::Inspector(control.0)
    } else if let Ok(control) = renderer_controls.get(change.source) {
        NumericScrubTarget::Renderer(control.0)
    } else {
        return;
    };
    let value = normalize_numeric_scrub_value(&session, target, change.value);
    commands.entity(change.source).insert(SliderValue(value));
    if let Ok(pair) = pairs.get(change.source) {
        commands.trigger(UpdateNumberInput {
            entity: pair.input,
            value: NumberInputValue::F32(value),
        });
    }

    if !change.is_final {
        let replace_active = state
            .active
            .is_some_and(|active| active.entity != change.source);
        if replace_active {
            session.restore_interaction_preview();
            state.active = None;
        }
        if state.active.is_none() {
            let Some(initial) = numeric_scrub_value(&session, target) else {
                return;
            };
            state.active = Some(ActiveBoundedSlider {
                entity: change.source,
                target,
                initial,
            });
        }
        preview_numeric_scrub(&mut session, target, value);
        return;
    }

    let (initial, target) = match state.active.take() {
        Some(active) if active.entity == change.source => (active.initial, active.target),
        Some(_) => {
            session.restore_interaction_preview();
            (
                numeric_scrub_value(&session, target).unwrap_or(value),
                target,
            )
        }
        None => (
            numeric_scrub_value(&session, target).unwrap_or(value),
            target,
        ),
    };
    if (value - initial).abs() <= f32::EPSILON {
        session.restore_interaction_preview();
    } else {
        commit_bounded_slider(&mut session, target, value);
    }
}

fn handle_emitter_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&EmitterNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let current = emitter_number_input_value(&session, *control);
    if (change.value - current).abs() <= f32::EPSILON {
        return;
    }
    match control {
        EmitterNumberControl::Start => {
            session.adjust_selected_start(change.value.max(0.0) - current);
        }
        EmitterNumberControl::Duration => {
            session.adjust_selected_duration(change.value.max(0.05) - current);
        }
        EmitterNumberControl::Translation(_)
        | EmitterNumberControl::Rotation(_)
        | EmitterNumberControl::Scale(_) => {
            set_emitter_transform_component(&mut session, *control, change.value, true);
        }
    }
}

fn handle_effect_clip_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&EffectClipNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let Some(current) = effect_clip_number_input_value(&session, *control) else {
        return;
    };
    if (change.value - current).abs() <= f32::EPSILON {
        return;
    }
    let Some(command) = effect_clip_transform_command(&session, *control, change.value) else {
        return;
    };
    session.execute("Transformed effect clip", command, true);
}

fn decorate_numeric_scrub_inputs(
    mut commands: Commands,
    children: Query<&Children>,
    inputs: Query<
        Entity,
        (
            Without<NumericScrubInput>,
            Or<(
                With<InspectorNumberControl>,
                With<EmitterNumberControl>,
                With<EffectClipNumberControl>,
                With<RendererNumberControl>,
            )>,
        ),
    >,
) {
    for entity in &inputs {
        commands.entity(entity).insert((
            NumericScrubInput,
            EntityCursor::System(SystemCursorIcon::EwResize),
        ));
        if let Ok(children) = children.get(entity) {
            for child in children.iter() {
                commands
                    .entity(child)
                    .insert(EntityCursor::System(SystemCursorIcon::EwResize));
            }
        }
    }
}

fn begin_numeric_scrub(
    mut drag: On<Pointer<DragStart>>,
    inspector_controls: Query<&InspectorNumberControl>,
    emitter_controls: Query<&EmitterNumberControl>,
    effect_clip_controls: Query<&EffectClipNumberControl>,
    renderer_controls: Query<&RendererNumberControl>,
    parents: Query<&ChildOf>,
    session: Res<EditorSession>,
    mut state: ResMut<NumericScrubState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<(&mut CursorIcon, &mut CursorOptions), With<PrimaryWindow>>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some((entity, target)) = resolve_numeric_scrub_target(
        drag.entity,
        &parents,
        &inspector_controls,
        &emitter_controls,
        &effect_clip_controls,
        &renderer_controls,
    ) else {
        return;
    };
    let Some(initial) = numeric_scrub_value(&session, target) else {
        return;
    };
    drag.propagate(false);
    state.active = Some(ActiveNumericScrub {
        entity,
        target,
        initial,
        raw: initial,
        current: initial,
    });
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::EwResize));
    *cursor.0 = CursorIcon::System(SystemCursorIcon::EwResize);
    cursor.1.visible = false;
}

fn update_numeric_scrub(
    mut drag: On<Pointer<Drag>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<NumericScrubState>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut text_inputs: Query<&mut EditableText>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(active) = state.active.as_mut() else {
        return;
    };
    if !numeric_scrub_event_belongs_to(drag.entity, active.entity, &parents) {
        return;
    }
    drag.propagate(false);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let multiplier = numeric_scrub_multiplier(shift, control);
    let delta = numeric_scrub_delta(drag.delta.x, numeric_scrub_step(active.target), multiplier);
    if delta == 0.0 {
        return;
    }
    active.raw += delta;
    active.current = normalize_numeric_scrub_value_with_multiplier(
        &session,
        active.target,
        active.raw,
        multiplier,
    );
    update_numeric_scrub_text(
        active.entity,
        format_numeric_scrub_value(active.target, active.current, multiplier),
        &children,
        &mut text_inputs,
    );
    preview_numeric_scrub(&mut session, active.target, active.current);
}

fn update_numeric_scrub_text(
    entity: Entity,
    value: String,
    children: &Query<&Children>,
    text_inputs: &mut Query<&mut EditableText>,
) {
    let Ok(children) = children.get(entity) else {
        return;
    };
    for child in children.iter() {
        let Ok(mut editable) = text_inputs.get_mut(child) else {
            continue;
        };
        editable.queue_edit(TextEdit::SelectAll);
        editable.queue_edit(TextEdit::Insert(value.into()));
        editable.queue_edit(TextEdit::CollapseSelection);
        break;
    }
}

fn finish_numeric_scrub(
    mut drag: On<Pointer<DragEnd>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<NumericScrubState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<(&mut CursorIcon, &mut CursorOptions), With<PrimaryWindow>>,
    parents: Query<&ChildOf>,
    localizer: Res<Localizer>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(active) = state.active.take() else {
        return;
    };
    if !numeric_scrub_event_belongs_to(drag.entity, active.entity, &parents) {
        state.active = Some(active);
        return;
    }
    drag.propagate(false);
    override_cursor.0 = None;
    *cursor.0 = CursorIcon::System(SystemCursorIcon::EwResize);
    cursor.1.visible = true;
    if (active.current - active.initial).abs() <= f32::EPSILON {
        session.restore_interaction_preview();
        return;
    }
    commit_numeric_scrub(&mut session, active.target, active.current, &localizer);
}

fn resolve_numeric_scrub_target(
    entity: Entity,
    parents: &Query<&ChildOf>,
    inspector_controls: &Query<&InspectorNumberControl>,
    emitter_controls: &Query<&EmitterNumberControl>,
    effect_clip_controls: &Query<&EffectClipNumberControl>,
    renderer_controls: &Query<&RendererNumberControl>,
) -> Option<(Entity, NumericScrubTarget)> {
    let mut candidate = entity;
    for _ in 0..4 {
        if let Ok(control) = inspector_controls.get(candidate) {
            return Some((candidate, NumericScrubTarget::Inspector(*control)));
        }
        if let Ok(control) = emitter_controls.get(candidate) {
            return Some((candidate, NumericScrubTarget::Emitter(*control)));
        }
        if let Ok(control) = effect_clip_controls.get(candidate) {
            return Some((candidate, NumericScrubTarget::EffectClip(*control)));
        }
        if let Ok(control) = renderer_controls.get(candidate) {
            return Some((candidate, NumericScrubTarget::Renderer(*control)));
        }
        candidate = parents.get(candidate).ok()?.parent();
    }
    None
}

fn numeric_scrub_event_belongs_to(
    entity: Entity,
    owner: Entity,
    parents: &Query<&ChildOf>,
) -> bool {
    let mut candidate = entity;
    for _ in 0..4 {
        if candidate == owner {
            return true;
        }
        let Ok(parent) = parents.get(candidate) else {
            return false;
        };
        candidate = parent.parent();
    }
    false
}

fn numeric_scrub_multiplier(shift: bool, control: bool) -> f32 {
    crate::feathers::number_input::scrub_multiplier(shift, control)
}

fn numeric_scrub_delta(pixel_delta: f32, step: f32, multiplier: f32) -> f32 {
    crate::feathers::number_input::scrub_delta(pixel_delta, step, multiplier)
}

fn numeric_scrub_step(target: NumericScrubTarget) -> f32 {
    match target {
        NumericScrubTarget::Inspector(control) => control.step,
        NumericScrubTarget::Emitter(control) => match control {
            EmitterNumberControl::Translation(_) => 0.1,
            EmitterNumberControl::Rotation(_) => 1.0,
            EmitterNumberControl::Scale(_) => 0.05,
            EmitterNumberControl::Start | EmitterNumberControl::Duration => 0.05,
        },
        NumericScrubTarget::EffectClip(control) => match control.control {
            EmitterNumberControl::Translation(_) => 0.1,
            EmitterNumberControl::Rotation(_) => 1.0,
            EmitterNumberControl::Scale(_) => 0.05,
            EmitterNumberControl::Start | EmitterNumberControl::Duration => 0.05,
        },
        NumericScrubTarget::Renderer(control) => renderer_number_step(control),
    }
}

fn renderer_number_step(control: RendererNumberControl) -> f32 {
    match control {
        RendererNumberControl::Softness(_) => 0.1,
        RendererNumberControl::Uv(_, _) => 0.05,
        RendererNumberControl::FlipbookFrameRate(_) => 1.0,
    }
}

fn numeric_scrub_value(session: &EditorSession, target: NumericScrubTarget) -> Option<f32> {
    match target {
        NumericScrubTarget::Inspector(control) => {
            inspector_number_input_value(session, control).map(number_input_value_as_f32)
        }
        NumericScrubTarget::Emitter(control) => Some(emitter_number_input_value(session, control)),
        NumericScrubTarget::EffectClip(control) => effect_clip_number_input_value(session, control),
        NumericScrubTarget::Renderer(control) => renderer_number_input_value(session, control),
    }
}

fn number_input_value_as_f32(value: NumberInputValue) -> f32 {
    match value {
        NumberInputValue::I32(value) => value as f32,
        NumberInputValue::F32(value) => value,
        NumberInputValue::I64(value) => value as f32,
        NumberInputValue::F64(value) => value as f32,
    }
}

fn format_numeric_scrub_value(target: NumericScrubTarget, value: f32, multiplier: f32) -> String {
    if matches!(
        target,
        NumericScrubTarget::Inspector(InspectorNumberControl {
            kind: InspectorNumberKind::U32,
            ..
        })
    ) {
        return (value.max(0.0).round().min(i32::MAX as f32) as i32).to_string();
    }
    let precision = numeric_scrub_precision(target, multiplier);
    crate::feathers::number_input::formatted(value, precision)
}

fn numeric_scrub_precision(target: NumericScrubTarget, multiplier: f32) -> usize {
    crate::feathers::number_input::decimal_places(numeric_scrub_step(target) * multiplier)
}

fn round_numeric_scrub_value(target: NumericScrubTarget, value: f32, multiplier: f32) -> f32 {
    if matches!(
        target,
        NumericScrubTarget::Inspector(InspectorNumberControl {
            kind: InspectorNumberKind::U32,
            ..
        })
    ) {
        return value.round();
    }
    crate::feathers::number_input::rounded(value, numeric_scrub_precision(target, multiplier))
}

fn normalize_numeric_scrub_value(
    session: &EditorSession,
    target: NumericScrubTarget,
    value: f32,
) -> f32 {
    normalize_numeric_scrub_value_with_multiplier(session, target, value, 1.0)
}

fn normalize_numeric_scrub_value_with_multiplier(
    session: &EditorSession,
    target: NumericScrubTarget,
    value: f32,
    multiplier: f32,
) -> f32 {
    if !value.is_finite() {
        return numeric_scrub_value(session, target).unwrap_or_default();
    }
    let normalized = match target {
        NumericScrubTarget::Inspector(control) => {
            let mut value =
                clamp_inspector_number(value, control.min, control.max).unwrap_or_default();
            if control.kind == InspectorNumberKind::U32 {
                value = value.max(0.0).round();
            } else if control.kind == InspectorNumberKind::Range
                && let Some(Value::Range(range)) =
                    inspector_module_parameter(session, control.module, control.parameter)
            {
                value = if control.component == 0 {
                    value.min(range.max)
                } else {
                    value.max(range.min)
                };
            }
            value
        }
        NumericScrubTarget::Emitter(EmitterNumberControl::Start) => {
            value.clamp(0.0, (session.effect.duration - 0.05).max(0.0))
        }
        NumericScrubTarget::Emitter(EmitterNumberControl::Duration) => value.clamp(
            0.05,
            (session.effect.duration - session.selected_layer().start_time).max(0.05),
        ),
        NumericScrubTarget::Emitter(EmitterNumberControl::Scale(_)) => value.max(0.001),
        NumericScrubTarget::Emitter(_) => value,
        NumericScrubTarget::EffectClip(EffectClipNumberControl {
            control: EmitterNumberControl::Scale(_),
            ..
        }) => value.max(0.001),
        NumericScrubTarget::EffectClip(_) => value,
        NumericScrubTarget::Renderer(RendererNumberControl::Softness(_)) => value.max(0.0),
        NumericScrubTarget::Renderer(RendererNumberControl::Uv(renderer, component)) => {
            normalize_renderer_uv_scrub_value(session, renderer, component, value)
        }
        NumericScrubTarget::Renderer(RendererNumberControl::FlipbookFrameRate(_)) => {
            value.clamp(1.0, 120.0)
        }
    };
    round_numeric_scrub_value(target, normalized, multiplier)
}

fn normalize_renderer_uv_scrub_value(
    session: &EditorSession,
    renderer: RendererId,
    component: u8,
    value: f32,
) -> f32 {
    let Some(material) = session
        .selected_layer()
        .renderers
        .iter()
        .find(|candidate| candidate.id == renderer)
        .and_then(|renderer| {
            session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)
        })
    else {
        return value.clamp(0.0, 1.0);
    };
    let MaterialProperties::Sprite { uv, .. } = &material.properties;
    match component {
        0 => value.clamp(0.0, uv.max[0]),
        1 => value.clamp(0.0, uv.max[1]),
        2 => value.clamp(uv.min[0], 1.0),
        3 => value.clamp(uv.min[1], 1.0),
        _ => value.clamp(0.0, 1.0),
    }
}

fn preview_numeric_scrub(
    session: &mut EditorSession,
    target: NumericScrubTarget,
    value: f32,
) -> bool {
    let Some(command) = numeric_scrub_command(session, target, value) else {
        return false;
    };
    session.preview_interaction(EffectTransaction::single("Preview numeric edit", command))
}

fn numeric_scrub_command(
    session: &EditorSession,
    target: NumericScrubTarget,
    value: f32,
) -> Option<EffectCommand> {
    match target {
        NumericScrubTarget::Inspector(control) => Some(EffectCommand::SetModuleParameter {
            emitter: session.selected_layer().id,
            module: control.module,
            parameter: control.parameter.into(),
            value: updated_inspector_number_value(session, control, value)?,
        }),
        NumericScrubTarget::Emitter(EmitterNumberControl::Start) => {
            let emitter = session.selected_layer();
            let start_time = normalize_numeric_scrub_value(session, target, value);
            Some(EffectCommand::SetEmitterTiming {
                id: emitter.id,
                start_time,
                duration: emitter.duration.min(session.effect.duration - start_time),
            })
        }
        NumericScrubTarget::Emitter(EmitterNumberControl::Duration) => {
            let emitter = session.selected_layer();
            Some(EffectCommand::SetEmitterTiming {
                id: emitter.id,
                start_time: emitter.start_time,
                duration: normalize_numeric_scrub_value(session, target, value),
            })
        }
        NumericScrubTarget::Emitter(control) => {
            let mut transform = session.selected_layer().transform;
            set_emitter_transform_value(&mut transform, control, value)?;
            Some(EffectCommand::SetEmitterTransform {
                id: session.selected_layer().id,
                transform,
            })
        }
        NumericScrubTarget::EffectClip(control) => {
            effect_clip_transform_command(session, control, value)
        }
        NumericScrubTarget::Renderer(control) => {
            renderer_numeric_scrub_command(session, control, value)
        }
    }
}

fn renderer_numeric_scrub_command(
    session: &EditorSession,
    control: RendererNumberControl,
    value: f32,
) -> Option<EffectCommand> {
    let renderer_id = match control {
        RendererNumberControl::Softness(id)
        | RendererNumberControl::Uv(id, _)
        | RendererNumberControl::FlipbookFrameRate(id) => id,
    };
    let renderer = session
        .selected_layer()
        .renderers
        .iter()
        .find(|renderer| renderer.id == renderer_id)?;
    match control {
        RendererNumberControl::Softness(_) | RendererNumberControl::Uv(_, _) => {
            let mut material = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)?
                .clone();
            let MaterialProperties::Sprite { softness, uv, .. } = &mut material.properties;
            match control {
                RendererNumberControl::Softness(_) => {
                    let MaterialInput::Constant(current) = softness else {
                        return None;
                    };
                    *current = value.max(0.0);
                }
                RendererNumberControl::Uv(_, component) => match component {
                    0 => uv.min[0] = value.clamp(0.0, uv.max[0]),
                    1 => uv.min[1] = value.clamp(0.0, uv.max[1]),
                    2 => uv.max[0] = value.clamp(uv.min[0], 1.0),
                    3 => uv.max[1] = value.clamp(uv.min[1], 1.0),
                    _ => return None,
                },
                RendererNumberControl::FlipbookFrameRate(_) => unreachable!(),
            }
            Some(EffectCommand::SetMaterial {
                id: material.id,
                material,
            })
        }
        RendererNumberControl::FlipbookFrameRate(_) => {
            let RendererProperties::Flipbook { flipbook, .. } = renderer.properties else {
                return None;
            };
            let mut definition = session
                .effect
                .flipbooks
                .iter()
                .find(|definition| definition.id == flipbook)?
                .clone();
            definition.frame_rate = value.clamp(1.0, 120.0);
            Some(EffectCommand::SetFlipbook {
                id: flipbook,
                flipbook: definition,
            })
        }
    }
}

fn commit_numeric_scrub(
    session: &mut EditorSession,
    target: NumericScrubTarget,
    value: f32,
    localizer: &Localizer,
) {
    match target {
        NumericScrubTarget::Inspector(control) => {
            apply_inspector_number(session, control, value, localizer);
        }
        NumericScrubTarget::Emitter(EmitterNumberControl::Start) => {
            let current = session.selected_layer().start_time;
            session.adjust_selected_start(value - current);
        }
        NumericScrubTarget::Emitter(EmitterNumberControl::Duration) => {
            let current = session.selected_layer().duration;
            session.adjust_selected_duration(value - current);
        }
        NumericScrubTarget::Emitter(control) => {
            set_emitter_transform_component(session, control, value, true);
        }
        NumericScrubTarget::EffectClip(control) => {
            if let Some(command) = effect_clip_transform_command(session, control, value) {
                session.execute("Transformed effect clip", command, true);
            }
        }
        NumericScrubTarget::Renderer(RendererNumberControl::Softness(renderer)) => {
            session.set_renderer_softness(renderer, value);
        }
        NumericScrubTarget::Renderer(RendererNumberControl::Uv(renderer, component)) => {
            session.set_renderer_uv(renderer, component, value);
        }
        NumericScrubTarget::Renderer(RendererNumberControl::FlipbookFrameRate(renderer)) => {
            session.set_flipbook_frame_rate(renderer, value);
        }
    }
}

fn commit_bounded_slider(
    session: &mut EditorSession,
    target: NumericScrubTarget,
    value: f32,
) -> bool {
    let Some(command) = numeric_scrub_command(session, target, value) else {
        return false;
    };
    let label = match target {
        NumericScrubTarget::Inspector(control) => format!("Changed {}", control.parameter),
        NumericScrubTarget::Renderer(RendererNumberControl::Uv(_, _)) => {
            "Changed material UV bounds".into()
        }
        NumericScrubTarget::Renderer(RendererNumberControl::FlipbookFrameRate(_)) => {
            "Changed flipbook frame rate".into()
        }
        NumericScrubTarget::Renderer(RendererNumberControl::Softness(_)) => {
            "Changed material softness".into()
        }
        NumericScrubTarget::Emitter(_) => "Changed emitter value".into(),
        NumericScrubTarget::EffectClip(_) => "Changed effect clip transform".into(),
    };
    session.execute(label, command, false)
}

fn handle_renderer_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&RendererToggleControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    match *control {
        RendererToggleControl::FlipbookLooping(renderer_id) => {
            let current = session
                .selected_layer()
                .renderers
                .iter()
                .find(|renderer| renderer.id == renderer_id)
                .and_then(|renderer| match renderer.properties {
                    RendererProperties::Flipbook { flipbook, .. } => session
                        .effect
                        .flipbooks
                        .iter()
                        .find(|definition| definition.id == flipbook)
                        .map(|definition| definition.looping),
                    _ => None,
                });
            if current.is_some_and(|current| current != change.value) {
                session.toggle_flipbook_looping(renderer_id);
            }
        }
        RendererToggleControl::FlipbookRandomStart(renderer_id) => {
            let current = session
                .selected_layer()
                .renderers
                .iter()
                .find(|renderer| renderer.id == renderer_id)
                .and_then(|renderer| match renderer.properties {
                    RendererProperties::Flipbook { random_start, .. } => Some(random_start),
                    _ => None,
                });
            if current.is_some_and(|current| current != change.value) {
                session.toggle_flipbook_random_start(renderer_id);
            }
        }
    }
}

fn handle_inspector_integer_change(
    change: On<ValueChange<i32>>,
    controls: Query<&InspectorNumberControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if control.kind != InspectorNumberKind::U32 {
        return;
    }
    apply_inspector_number(&mut session, *control, change.value as f32, &localizer);
}

fn handle_inspector_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&InspectorNumberControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if control.kind == InspectorNumberKind::U32 {
        return;
    }
    apply_inspector_number(&mut session, *control, change.value, &localizer);
}

fn inspector_module_parameter(
    session: &EditorSession,
    module: ModuleId,
    parameter: &str,
) -> Option<Value> {
    let module = session
        .selected_layer()
        .modules
        .iter()
        .find(|candidate| candidate.id == module)?;
    module_parameter(module, parameter)
}

fn apply_inspector_number(
    session: &mut EditorSession,
    control: InspectorNumberControl,
    raw_value: f32,
    localizer: &Localizer,
) -> bool {
    let Some(value) = clamp_inspector_number(raw_value, control.min, control.max) else {
        set_inspector_status(
            session,
            localizer,
            InspectorStatus::FiniteNumberRequired(control.parameter.into()),
        );
        return false;
    };
    let Some(current) = inspector_module_parameter(session, control.module, control.parameter)
    else {
        set_inspector_status(session, localizer, InspectorStatus::TargetUnavailable);
        return false;
    };
    let Some(updated) = updated_inspector_number_value(session, control, value) else {
        set_inspector_status(
            session,
            localizer,
            InspectorStatus::IncompatibleMetadata(control.parameter.into()),
        );
        return false;
    };
    if updated == current {
        return false;
    }
    session.set_module_parameter(control.module, control.parameter, updated);
    true
}

fn updated_inspector_number_value(
    session: &EditorSession,
    control: InspectorNumberControl,
    raw_value: f32,
) -> Option<Value> {
    let value = clamp_inspector_number(raw_value, control.min, control.max)?;
    let current = inspector_module_parameter(session, control.module, control.parameter)?;
    match (control.kind, current) {
        (InspectorNumberKind::U32, Value::U32(_)) => Some(Value::U32(
            value.max(0.0).round().min(u32::MAX as f32) as u32,
        )),
        (InspectorNumberKind::Scalar, Value::Scalar(_)) => Some(Value::Scalar(value)),
        (InspectorNumberKind::Vector, Value::Vec2(mut vector)) => {
            let component = vector.get_mut(control.component as usize)?;
            *component = value;
            Some(Value::Vec2(vector))
        }
        (InspectorNumberKind::Vector, Value::Vec3(mut vector)) => {
            let component = vector.get_mut(control.component as usize)?;
            *component = value;
            Some(Value::Vec3(vector))
        }
        (InspectorNumberKind::Vector, Value::Vec4(mut vector)) => {
            let component = vector.get_mut(control.component as usize)?;
            *component = value;
            Some(Value::Vec4(vector))
        }
        (InspectorNumberKind::Range, Value::Range(mut range)) => {
            if control.component == 0 {
                range.min = value.min(range.max);
            } else {
                range.max = value.max(range.min);
            }
            Some(Value::Range(range))
        }
        (InspectorNumberKind::Shape, Value::Shape(shape)) => {
            shape_with_dimension(shape, control.component, value).map(Value::Shape)
        }
        _ => None,
    }
}

fn clamp_inspector_number(value: f32, min: Option<f32>, max: Option<f32>) -> Option<f32> {
    if !value.is_finite() {
        return None;
    }
    let value = min.map_or(value, |min| value.max(min));
    Some(max.map_or(value, |max| value.min(max)))
}

fn spawn_read_only_inspector_shell(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    localizer: &Localizer,
    instance_editable: bool,
    body: impl FnOnce(&mut ChildSpawnerCommands),
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
                &localizer.text("inspector-referenced-effect-heading"),
                &localizer.text(if instance_editable {
                    "inspector-instance-editable"
                } else {
                    "inspector-read-only"
                }),
            );
            panel.spawn((
                Text::new(title),
                InspectorTitle,
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Node {
                    margin: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                    ..default()
                },
            ));
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|container| {
                    spawn_vertical_scroll_area(
                        container,
                        ScrollMemoryKey::Inspector,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(7.0),
                            padding: UiRect::all(Val::Px(8.0)),
                            ..default()
                        },
                        body,
                    );
                });
        });
}

fn spawn_read_only_card(
    parent: &mut ChildSpawnerCommands,
    heading: impl Into<String>,
    body: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(heading),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            body(card);
        });
}

fn spawn_read_only_row(
    parent: &mut ChildSpawnerCommands,
    label: impl Into<String>,
    value: impl Into<String>,
) -> Entity {
    let mut value_entity = None;
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            align_items: AlignItems::Start,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Node {
                    width: Val::Px(92.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            value_entity = Some(
                row.spawn((
                    Text::new(value),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    Node {
                        min_width: Val::Px(0.0),
                        flex_grow: 1.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .id(),
            );
        });
    value_entity.expect("read-only rows always spawn a value")
}

#[derive(Component, Clone, Copy)]
struct EffectClipInspectorTimingText {
    clip: EffectClipId,
    field: EffectClipInspectorTimingField,
}

#[derive(Clone, Copy)]
enum EffectClipInspectorTimingField {
    Start,
    SourceOffset,
    Duration,
}

fn sync_effect_clip_inspector_timing(
    session: Res<EditorSession>,
    timeline: Option<Res<TimelineState>>,
    mut texts: Query<(&EffectClipInspectorTimingText, &mut Text)>,
) {
    for (marker, mut text) in &mut texts {
        let timing = timeline
            .as_ref()
            .and_then(|state| state.effect_clip_preview_timing(marker.clip))
            .or_else(|| {
                session
                    .effect
                    .effect_clips
                    .iter()
                    .find(|clip| clip.id == marker.clip)
                    .map(|clip| (clip.start_time, clip.source_offset, clip.duration))
            });
        let Some((start, source_offset, duration)) = timing else {
            continue;
        };
        let value = match marker.field {
            EffectClipInspectorTimingField::Start => start,
            EffectClipInspectorTimingField::SourceOffset => source_offset,
            EffectClipInspectorTimingField::Duration => duration,
        };
        text.0 = format!("{value:.3} s");
    }
}

fn effect_clip_catalog_name(catalog: &ProjectEffectCatalog, source: EffectAssetRef) -> String {
    catalog
        .entries()
        .iter()
        .find(|entry| entry.reference == Some(source))
        .map_or_else(|| source.id.to_string(), |entry| entry.display_name.clone())
}

fn effect_clip_repair_source(
    catalog: &ProjectEffectCatalog,
    owner: &EffectAsset,
    clip: &EffectClip,
    source: EffectAssetRef,
) -> Result<EffectAsset, String> {
    if source == clip.source {
        return Err("select a different source effect".into());
    }
    let source_effect = catalog.effect_for_placement(owner, source)?;
    let source_end = clip.source_offset + clip.duration;
    if !source_effect.looping && source_end > source_effect.duration + f32::EPSILON {
        return Err(format!(
            "the clip window ends at {source_end:.3} s, beyond the source duration of {:.3} s",
            source_effect.duration
        ));
    }
    Ok(source_effect)
}

fn repair_candidate_matches(query: &str, name: &str, path: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || name.to_lowercase().contains(&query) || path.to_lowercase().contains(&query)
}

fn spawn_effect_clip_repair(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    repair: &EffectClipRepairState,
    localizer: &Localizer,
    clip: &EffectClip,
    dependency_error: &str,
) {
    spawn_read_only_card(
        parent,
        localizer.text("inspector-repair-reference"),
        |card| {
            card.spawn((
                Text::new(dependency_error),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
                Pickable::IGNORE,
            ));
            card.spawn(Node {
                width: Val::Percent(100.0),
                ..default()
            })
            .with_children(|search| {
                spawn_search_field(
                    search,
                    &repair.query,
                    &localizer.text("inspector-repair-search-placeholder"),
                    &localizer.text("inspector-repair-search-clear"),
                    EffectClipRepairSearchInput,
                );
            });

            let mut compatible = 0;
            for entry in catalog.entries() {
                let Some(reference) = entry.reference else {
                    continue;
                };
                if effect_clip_repair_source(catalog, &session.effect, clip, reference).is_err() {
                    continue;
                }
                let path = entry.path.display().to_string();
                let visible = repair_candidate_matches(&repair.query, &entry.display_name, &path);
                let accessible = format!(
                    "{} {}",
                    localizer.text("inspector-repair-reference"),
                    entry.display_name
                );
                let row = spawn_action_list_row(
                    card,
                    &entry.display_name,
                    Some(&path),
                    None,
                    &accessible,
                    InspectorAction::RepairEffectClipSource {
                        clip: clip.id,
                        source: reference,
                    },
                );
                card.commands()
                    .entity(row)
                    .insert(EffectClipRepairCandidate {
                        search_text: format!("{} {}", entry.display_name, path).to_lowercase(),
                    });
                compatible += usize::from(visible);
            }
            let empty = spawn_list_empty_state(
                card,
                &localizer.text("inspector-repair-no-results-title"),
                &localizer.text("inspector-repair-no-results-message"),
                theme::TEXT_MUTED,
                if compatible == 0 {
                    Display::Flex
                } else {
                    Display::None
                },
            );
            card.commands().entity(empty).insert(EffectClipRepairEmpty);
        },
    );
}

fn spawn_effect_clip_inspector(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    repair: &EffectClipRepairState,
    localizer: &Localizer,
    id: EffectClipId,
) -> bool {
    let Some(clip) = session
        .effect
        .effect_clips
        .iter()
        .find(|clip| clip.id == id)
    else {
        return false;
    };
    let source_name = effect_clip_catalog_name(catalog, clip.source);
    let source = catalog.load_effect(clip.source).ok();
    let dependency_error = catalog.effect_clip_dependency_error(&session.effect, clip.id);
    spawn_read_only_inspector_shell(parent, &source_name, localizer, true, |stack| {
        spawn_read_only_card(stack, localizer.text("inspector-effect-clip"), |card| {
            spawn_read_only_row(card, localizer.text("inspector-source"), &source_name);
            let start = spawn_read_only_row(
                card,
                localizer.text("inspector-start"),
                format!("{:.3} s", clip.start_time),
            );
            card.commands()
                .entity(start)
                .insert(EffectClipInspectorTimingText {
                    clip: clip.id,
                    field: EffectClipInspectorTimingField::Start,
                });
            let source_offset = spawn_read_only_row(
                card,
                localizer.text("inspector-source-offset"),
                format!("{:.3} s", clip.source_offset),
            );
            card.commands()
                .entity(source_offset)
                .insert(EffectClipInspectorTimingText {
                    clip: clip.id,
                    field: EffectClipInspectorTimingField::SourceOffset,
                });
            let duration = spawn_read_only_row(
                card,
                localizer.text("inspector-duration"),
                format!("{:.3} s", clip.duration),
            );
            card.commands()
                .entity(duration)
                .insert(EffectClipInspectorTimingText {
                    clip: clip.id,
                    field: EffectClipInspectorTimingField::Duration,
                });
            spawn_read_only_row(
                card,
                localizer.text("inspector-seed"),
                format!("{:?}", clip.seed),
            );
        });
        spawn_effect_clip_transform_controls(stack, clip.id);
        if let Some(error) = dependency_error.as_deref() {
            spawn_effect_clip_repair(stack, session, catalog, repair, localizer, clip, error);
        }
        spawn_read_only_card(stack, localizer.text("inspector-source-summary"), |card| {
            if let Some(source) = &source {
                spawn_read_only_row(card, localizer.text("inspector-name"), &source.name);
                spawn_read_only_row(
                    card,
                    localizer.text("inspector-duration"),
                    format!("{:.3} s", source.duration),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("inspector-emitters"),
                    source.emitters.len().to_string(),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("inspector-looping"),
                    source.looping.to_string(),
                );
            } else {
                spawn_read_only_row(
                    card,
                    localizer.text("inspector-status"),
                    localizer.text("inspector-source-unavailable"),
                );
            }
        });
        stack.spawn((
            Text::new(localizer.text("inspector-effect-clip-read-only-description")),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
    });
    true
}

fn spawn_referenced_emitter_inspector(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    path: &EffectClipPath,
    selected_emitter: EmitterId,
) -> bool {
    let Some((clip, source)) = resolve_effect_clip_path(session, catalog, path) else {
        return false;
    };
    let Some(emitter) = source
        .emitters
        .iter()
        .find(|emitter| emitter.id == selected_emitter)
    else {
        return false;
    };
    let source_name = effect_clip_catalog_name(catalog, clip.source);
    spawn_read_only_inspector_shell(parent, &emitter.name, localizer, false, |stack| {
        spawn_read_only_card(stack, localizer.text("inspector-reference"), |card| {
            spawn_read_only_row(card, localizer.text("inspector-source"), &source_name);
            spawn_read_only_row(card, localizer.text("inspector-emitter"), &emitter.name);
            spawn_read_only_row(
                card,
                localizer.text("inspector-mode"),
                localizer.text("inspector-read-only"),
            );
        });
        spawn_read_only_card(stack, localizer.text("inspector-emitter"), |card| {
            spawn_read_only_row(
                card,
                localizer.text("inspector-enabled"),
                emitter.enabled.to_string(),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-capacity"),
                emitter.max_particles.to_string(),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-start"),
                format!("{:.3} s", emitter.start_time),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-duration"),
                format!("{:.3} s", emitter.duration),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-domain"),
                format!("{:?}", emitter.simulation_domain),
            );
        });
        spawn_read_only_card(stack, localizer.text("inspector-transform"), |card| {
            spawn_read_only_row(
                card,
                localizer.text("inspector-position"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    emitter.transform.translation[0],
                    emitter.transform.translation[1],
                    emitter.transform.translation[2]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-rotation"),
                format!(
                    "{:.3}, {:.3}, {:.3}, {:.3}",
                    emitter.transform.rotation[0],
                    emitter.transform.rotation[1],
                    emitter.transform.rotation[2],
                    emitter.transform.rotation[3]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-scale"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    emitter.transform.scale[0],
                    emitter.transform.scale[1],
                    emitter.transform.scale[2]
                ),
            );
        });
        for module in &emitter.modules {
            spawn_read_only_card(stack, &module.module_type.0, |card| {
                spawn_read_only_row(
                    card,
                    localizer.text("inspector-stage"),
                    format!("{:?}", module.stage),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("inspector-enabled"),
                    module.enabled.to_string(),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("inspector-parameters"),
                    format!("{:?}", module.parameters),
                );
            });
        }
        for renderer in &emitter.renderers {
            spawn_read_only_card(stack, &renderer.renderer_type.0, |card| {
                spawn_read_only_row(
                    card,
                    localizer.text("inspector-enabled"),
                    renderer.enabled.to_string(),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("inspector-properties"),
                    format!("{:?}", renderer.properties),
                );
            });
        }
    });
    true
}

fn spawn_referenced_effect_clip_inspector(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    path: &EffectClipPath,
) -> bool {
    let Some((clip, source)) = resolve_effect_clip_path(session, catalog, path) else {
        return false;
    };
    let source_name = effect_clip_catalog_name(catalog, clip.source);
    spawn_read_only_inspector_shell(parent, &source_name, localizer, false, |stack| {
        spawn_read_only_card(stack, localizer.text("inspector-effect-clip"), |card| {
            spawn_read_only_row(card, localizer.text("inspector-source"), &source_name);
            spawn_read_only_row(
                card,
                localizer.text("inspector-start"),
                format!("{:.3} s", clip.start_time),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-source-offset"),
                format!("{:.3} s", clip.source_offset),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-duration"),
                format!("{:.3} s", clip.duration),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-seed"),
                format!("{:?}", clip.seed),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-mode"),
                localizer.text("inspector-read-only"),
            );
        });
        spawn_read_only_card(stack, localizer.text("inspector-transform"), |card| {
            spawn_read_only_row(
                card,
                localizer.text("inspector-position"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    clip.transform.translation[0],
                    clip.transform.translation[1],
                    clip.transform.translation[2]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-rotation"),
                format!(
                    "{:.3}, {:.3}, {:.3}, {:.3}",
                    clip.transform.rotation[0],
                    clip.transform.rotation[1],
                    clip.transform.rotation[2],
                    clip.transform.rotation[3]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-scale"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    clip.transform.scale[0], clip.transform.scale[1], clip.transform.scale[2]
                ),
            );
        });
        spawn_read_only_card(stack, localizer.text("inspector-source-summary"), |card| {
            spawn_read_only_row(card, localizer.text("inspector-name"), &source.name);
            spawn_read_only_row(
                card,
                localizer.text("inspector-duration"),
                format!("{:.3} s", source.duration),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-emitters"),
                source.emitters.len().to_string(),
            );
            spawn_read_only_row(
                card,
                localizer.text("inspector-looping"),
                source.looping.to_string(),
            );
        });
        stack.spawn((
            Text::new(localizer.text("inspector-effect-clip-read-only-description")),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
    });
    true
}

pub(crate) fn spawn_inspector(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    palette: &ModulePaletteState,
    localizer: &Localizer,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    timeline: &TimelineState,
    repair: &EffectClipRepairState,
) {
    if let Some(selection) = timeline.inspected_child.as_ref() {
        let spawned = match selection {
            EffectClipChildSelection::EffectClip { path } => {
                spawn_referenced_effect_clip_inspector(parent, session, catalog, localizer, path)
            }
            EffectClipChildSelection::Emitter { path, emitter } => {
                spawn_referenced_emitter_inspector(
                    parent, session, catalog, localizer, path, *emitter,
                )
            }
        };
        if spawned {
            return;
        }
    }
    if let SemanticTarget::EffectClip(clip) = session.selection.primary
        && spawn_effect_clip_inspector(parent, session, catalog, repair, localizer, clip)
    {
        return;
    }
    let layer = session.selected_layer();
    let emitter_index = session.selected_layer_index();
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel_heading(panel, "MODULE STACK", "LIVE COMPILE");
            panel.spawn((
                Text::new(&layer.name),
                InspectorTitle,
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Node {
                    margin: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                    ..default()
                },
            ));
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::Inspector,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::bottom(Val::Px(12.0)),
                            ..default()
                        },
                        |stack| {
                    spawn_document_controls(stack, session, localizer);
                    spawn_inspector_parameters(stack, session);
                    spawn_emitter_transform_controls(stack);
                    spawn_emitter_timing_controls(stack);
                    spawn_event_links(stack, session, localizer);
                    for stage in StackStage::ALL {
                        spawn_stage_header(stack, stage);
                        if stage == StackStage::Render {
                            for (renderer_index, renderer) in layer.renderers.iter().enumerate() {
                                spawn_renderer_card(
                                    stack,
                                    renderer,
                                    &format!(
                                        "effect.emitters[{emitter_index}].renderers[{renderer_index}]"
                                    ),
                                    session,
                                    inspector_renderer_collapsed(settings, renderer),
                                );
                            }
                            spawn_stage_diagnostics(
                                stack,
                                stage,
                                &format!("effect.emitters[{emitter_index}].renderers"),
                                session,
                                registry,
                            );
                            continue;
                        }
                        let semantic = stage.semantic().expect("module stage has semantics");
                        for (module_index, module) in layer.modules.iter().enumerate() {
                            if module.stage != semantic {
                                continue;
                            }
                            spawn_module_card(
                                stack,
                                module,
                                registry.0.get(&module.module_type),
                                &format!(
                                    "effect.emitters[{emitter_index}].modules[{module_index}]"
                                ),
                                session,
                                localizer,
                                inspector_module_collapsed(settings, module),
                            );
                        }
                        spawn_stage_diagnostics(
                            stack,
                            stage,
                            &format!("effect.emitters[{emitter_index}].modules"),
                            session,
                            registry,
                        );
                    }
                        },
                    );
                });
            if palette.open {
                spawn_module_palette(panel, registry, palette);
            }
        });
}

fn spawn_document_controls(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(7.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(localizer.text("inspector-effect")),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            spawn_text_field(
                card,
                &localizer.text("inspector-effect-name"),
                &localizer.text("inspector-effect-name-description"),
                &session.effect.name,
                DocumentTextControl::EffectName,
            );
        });

    let emitter = session.selected_layer();
    parent
        .spawn((
            InspectorSemanticTarget {
                target: SemanticTarget::Emitter(emitter.id),
                base_border: theme::BORDER,
            },
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(7.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(localizer.text("inspector-emitter")),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            spawn_text_field(
                card,
                &localizer.text("inspector-emitter-name"),
                &localizer.text("inspector-emitter-name-description"),
                &emitter.name,
                DocumentTextControl::EmitterName,
            );
            spawn_document_toggle(
                card,
                &localizer.text("inspector-emitter-enabled"),
                &localizer.text("inspector-emitter-enabled-description"),
                emitter.enabled,
                DocumentToggleControl::EmitterEnabled,
            );
            crate::feathers::field_row::spawn_field_row(
                card,
                crate::feathers::field_row::FieldRowProps::new(
                    localizer.text("inspector-emitter-capacity"),
                )
                .with_control_min_width(150.0),
                EditorTooltip::description(
                    localizer.text("inspector-emitter-capacity-description"),
                ),
                |controls| {
                    controls
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_integer_input())
                        .insert((
                            EmitterCapacityControl,
                            AccessibleLabel(localizer.text("inspector-emitter-capacity")),
                        ));
                },
            );
        });
}

fn spawn_text_field(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    description: &str,
    value: &str,
    control: DocumentTextControl,
) {
    crate::feathers::field_row::spawn_field_row(
        parent,
        crate::feathers::field_row::FieldRowProps::new(title).with_control_min_width(150.0),
        EditorTooltip::description(description.to_owned()),
        |inputs| {
            spawn_text_input(inputs, value, title, control);
        },
    );
}

fn spawn_document_toggle(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    description: &str,
    enabled: bool,
    control: DocumentToggleControl,
) {
    crate::feathers::field_row::spawn_field_row(
        parent,
        crate::feathers::field_row::FieldRowProps::new(title).with_control_min_width(150.0),
        EditorTooltip::description(description.to_owned()),
        |inputs| {
            let mut checkbox = inputs.spawn_empty();
            checkbox
                .apply_scene(ui_shell::feathers_checkbox())
                .insert((control, AccessibleLabel(title.to_owned())));
            if enabled {
                checkbox.insert(Checked);
            }
        },
    );
}

fn spawn_event_links(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    let source = session.selected_layer().id;
    parent.spawn((
        Text::new(localizer.text("inspector-events")),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::ACCENT),
        Node {
            margin: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
            ..default()
        },
    ));

    let outgoing = session
        .effect
        .events
        .iter()
        .filter(|event| event.source == source)
        .collect::<Vec<_>>();
    if outgoing.is_empty() {
        parent
            .spawn_empty()
            .apply_scene(label_dim(localizer.text("inspector-events-empty")));
    }
    for event in outgoing {
        let target = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == event.target)
            .map_or_else(|| event.target.to_string(), |emitter| emitter.name.clone());
        let mut args = FluentArgs::new();
        args.set("trigger", localized_event_trigger(localizer, event.trigger));
        args.set("target", target);
        parent
            .spawn((
                InspectorSemanticTarget {
                    target: SemanticTarget::Event(event.id),
                    base_border: theme::BORDER,
                },
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(30.0),
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL_LIGHT),
                BorderColor::all(theme::BORDER),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(localizer.text_with("inspector-event-link", &args)),
                    ThemedText,
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ));
                mini_button(row, "×", InspectorAction::DeleteEventLink(event.id));
            });
    }

    let mut options = Vec::new();
    for target in session
        .effect
        .emitters
        .iter()
        .filter(|emitter| emitter.id != source)
    {
        for trigger in [
            EventTrigger::OnSpawn,
            EventTrigger::OnDeath,
            EventTrigger::OnCollision,
        ] {
            if session.effect.events.iter().any(|event| {
                event.source == source && event.target == target.id && event.trigger == trigger
            }) {
                continue;
            }
            let mut args = FluentArgs::new();
            args.set("trigger", localized_event_trigger(localizer, trigger));
            args.set("target", target.name.clone());
            options.push(ComboOption {
                label: localizer.text_with("inspector-event-link", &args),
                selected: false,
                action: InspectorAction::AddEventLink {
                    trigger,
                    target: target.id,
                },
            });
        }
    }
    if options.is_empty() {
        parent
            .spawn_empty()
            .apply_scene(label_dim(localizer.text("inspector-events-no-targets")));
    } else {
        parent
            .spawn(Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(5.0)),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|row| {
                spawn_combo_control(
                    row,
                    &localizer.text("inspector-events-add"),
                    &localizer.text("inspector-events-add-description"),
                    &options,
                    230.0,
                );
            });
    }
}

fn localized_event_trigger(localizer: &Localizer, trigger: EventTrigger) -> String {
    localizer.text(match trigger {
        EventTrigger::OnSpawn => "inspector-event-on-spawn",
        EventTrigger::OnDeath => "inspector-event-on-death",
        EventTrigger::OnCollision => "inspector-event-on-collision",
    })
}

fn inspector_module_collapsed(settings: &EditorSettings, module: &ModuleInstance) -> bool {
    inspector_module_card_memory(module).collapsed(&settings.inspector.section_expansion)
}

fn inspector_renderer_collapsed(
    settings: &EditorSettings,
    renderer: &aestra_bevy::RendererInstance,
) -> bool {
    inspector_renderer_card_memory(renderer).collapsed(&settings.inspector.section_expansion)
}

fn inspector_module_card_memory(module: &ModuleInstance) -> RememberedPanelCard {
    RememberedPanelCard::new(
        inspector_module_key(module),
        !matches!(module.stage, StageKind::ParticleUpdate),
    )
}

fn inspector_renderer_card_memory(renderer: &aestra_bevy::RendererInstance) -> RememberedPanelCard {
    RememberedPanelCard::new(inspector_renderer_key(renderer), false)
}

fn inspector_module_key(module: &ModuleInstance) -> String {
    format!("module/{}", module.module_type.0)
}

fn inspector_renderer_key(renderer: &aestra_bevy::RendererInstance) -> String {
    match renderer.properties {
        RendererProperties::Sprite => "renderer/sprite",
        RendererProperties::Flipbook { .. } => "renderer/flipbook",
        _ => "renderer/unknown",
    }
    .into()
}

pub(crate) fn toggle_persisted_inspector_section(
    session: &EditorSession,
    settings: &mut EditorSettings,
    section: InspectorSection,
) -> bool {
    let card = match section {
        InspectorSection::Module(id) => {
            let Some(module) = session
                .selected_layer()
                .modules
                .iter()
                .find(|module| module.id == id)
            else {
                return false;
            };
            inspector_module_card_memory(module)
        }
        InspectorSection::Renderer(id) => {
            let Some(renderer) = session
                .selected_layer()
                .renderers
                .iter()
                .find(|renderer| renderer.id == id)
            else {
                return false;
            };
            inspector_renderer_card_memory(renderer)
        }
    };
    card.toggle(&mut settings.inspector.section_expansion);
    true
}

fn spawn_emitter_timing_controls(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EditorTooltip::description("Start offset and active duration for this emitter."),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(29.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(5.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for (title, control) in [
                ("Start", EmitterNumberControl::Start),
                ("Duration", EmitterNumberControl::Duration),
            ] {
                row.spawn_empty().apply_scene(label(title));
                row.spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(62.0),
                    ..default()
                })
                .with_children(|input| {
                    input
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((control, AccessibleLabel(format!("{title} in seconds"))));
                });
                row.spawn_empty().apply_scene(label_dim("s"));
            }
        });
}

fn spawn_emitter_transform_controls(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(7.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new("Transform"),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            spawn_emitter_transform_row(
                card,
                "Position",
                "Emitter origin in effect-local units.",
                EmitterNumberControl::Translation,
            );
            spawn_emitter_transform_row(
                card,
                "Rotation",
                "Emitter orientation in local Euler degrees.",
                EmitterNumberControl::Rotation,
            );
            spawn_emitter_transform_row(
                card,
                "Scale",
                "Emitter simulation and renderer scale.",
                EmitterNumberControl::Scale,
            );
        });
}

fn spawn_effect_clip_transform_controls(parent: &mut ChildSpawnerCommands, clip: EffectClipId) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(7.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new("Instance Transform"),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            for (title, description, constructor) in [
                (
                    "Position",
                    "Offset the complete referenced effect in parent-effect units.",
                    EmitterNumberControl::Translation as fn(u8) -> EmitterNumberControl,
                ),
                (
                    "Rotation",
                    "Rotate the complete referenced effect in Euler degrees.",
                    EmitterNumberControl::Rotation,
                ),
                (
                    "Scale",
                    "Scale the complete referenced effect instance.",
                    EmitterNumberControl::Scale,
                ),
            ] {
                spawn_effect_clip_transform_row(card, clip, title, description, constructor);
            }
        });
}

fn spawn_effect_clip_transform_row(
    parent: &mut ChildSpawnerCommands,
    clip: EffectClipId,
    title: &str,
    description: &str,
    control: fn(u8) -> EmitterNumberControl,
) {
    crate::feathers::field_row::spawn_field_row(
        parent,
        crate::feathers::field_row::FieldRowProps::new(title)
            .indented(0)
            .with_control_min_width(150.0),
        EditorTooltip::description(description),
        |inputs| {
            for (axis, component, color) in [
                ("X", 0, tokens::TEXT_INPUT_X_AXIS),
                ("Y", 1, tokens::TEXT_INPUT_Y_AXIS),
                ("Z", 2, tokens::TEXT_INPUT_Z_AXIS),
            ] {
                inputs
                    .spawn(Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_width: Val::Px(44.0),
                        ..default()
                    })
                    .with_children(|wrapper| {
                        wrapper
                            .spawn_empty()
                            .apply_scene(ui_shell::feathers_labeled_scalar_input(axis, color))
                            .insert((
                                EffectClipNumberControl {
                                    clip,
                                    control: control(component),
                                },
                                AccessibleLabel(format!("{title} {axis}")),
                            ));
                    });
            }
        },
    );
}

fn spawn_emitter_transform_row(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    description: &str,
    control: fn(u8) -> EmitterNumberControl,
) {
    crate::feathers::field_row::spawn_field_row(
        parent,
        crate::feathers::field_row::FieldRowProps::new(title)
            .indented(0)
            .with_control_min_width(150.0),
        EditorTooltip::description(description),
        |inputs| {
            for (axis, component, color) in [
                ("X", 0, tokens::TEXT_INPUT_X_AXIS),
                ("Y", 1, tokens::TEXT_INPUT_Y_AXIS),
                ("Z", 2, tokens::TEXT_INPUT_Z_AXIS),
            ] {
                inputs
                    .spawn(Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_width: Val::Px(44.0),
                        ..default()
                    })
                    .with_children(|wrapper| {
                        wrapper
                            .spawn_empty()
                            .apply_scene(ui_shell::feathers_labeled_scalar_input(axis, color))
                            .insert((
                                control(component),
                                AccessibleLabel(format!("{title} {axis}")),
                            ));
                    });
            }
        },
    );
}

fn spawn_stage_header(parent: &mut ChildSpawnerCommands, stage: StackStage) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                margin: UiRect::top(Val::Px(3.0)),
                align_items: AlignItems::Center,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(stage.title()),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            mini_button(row, "+", InspectorAction::OpenModulePalette(stage));
        });
}

fn spawn_inspector_parameters(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    if session.effect.parameters.is_empty() {
        return;
    }
    parent.spawn((
        Text::new("EFFECT PARAMETERS"),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::ACCENT),
        Node {
            margin: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
            ..default()
        },
    ));
    for parameter in &session.effect.parameters {
        parent
            .spawn((
                InspectorSemanticTarget {
                    target: SemanticTarget::Parameter(parameter.id),
                    base_border: theme::BORDER_BRIGHT,
                },
                Node {
                    width: Val::Auto,
                    min_height: Val::Px(34.0),
                    margin: UiRect::axes(Val::Px(9.0), Val::Px(3.0)),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL_LIGHT),
                BorderColor::all(theme::BORDER_BRIGHT),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(&parameter.name),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(theme::TEXT),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                row.spawn((
                    Text::new(format_value(parameter.default.clone())),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                ));
            });
    }
}

fn spawn_module_card(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    metadata: Option<&ModuleMetadata>,
    diagnostic_path: &str,
    session: &EditorSession,
    localizer: &Localizer,
    collapsed: bool,
) {
    let display_name = metadata.map_or(module.module_type.0.as_str(), |item| item.display_name);
    let help = metadata.map_or(
        "This module is not available in the current registry.",
        |item| item.description,
    );
    let base_border = if session
        .diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.path.starts_with(diagnostic_path))
    {
        Color::srgb(0.82, 0.28, 0.24)
    } else if session.selection.primary == SemanticTarget::Module(module.id) {
        theme::ACCENT_DIM
    } else {
        theme::BORDER
    };
    spawn_remembered_panel_card(
        parent,
        PanelCardProps::new(display_name, collapsed)
            .with_memory_key(inspector_module_key(module))
            .with_help(help)
            .with_enabled(module.enabled)
            .with_background(if module.enabled {
                theme::PANEL_LIGHT
            } else {
                theme::PANEL_DARK
            })
            .with_border(base_border),
        InspectorSemanticTarget {
            target: SemanticTarget::Module(module.id),
            base_border,
        },
        InspectorSelectionTarget(SemanticTarget::Module(module.id)),
        InspectorAction::ToggleSection(InspectorSection::Module(module.id)),
        |header| {
            let mut enabled = header.spawn_empty();
            enabled.apply_scene(ui_shell::feathers_checkbox()).insert((
                ModuleEnabledControl(module.id),
                AccessibleLabel(format!("Enable {display_name}")),
            ));
            if module.enabled {
                enabled.insert(Checked);
            }
            spawn_action_menu(
                header,
                &format!("{display_name} actions"),
                &[
                    ComboOption {
                        label: "Move up".into(),
                        selected: false,
                        action: InspectorAction::MoveModule(module.id, -1),
                    },
                    ComboOption {
                        label: "Move down".into(),
                        selected: false,
                        action: InspectorAction::MoveModule(module.id, 1),
                    },
                    ComboOption {
                        label: "Duplicate".into(),
                        selected: false,
                        action: InspectorAction::DuplicateModule(module.id),
                    },
                    ComboOption {
                        label: "Delete…".into(),
                        selected: false,
                        action: InspectorAction::DeleteModule(module.id),
                    },
                ],
            );
        },
        |card| {
            if let Some(metadata) = metadata {
                for (input_index, input) in metadata.inputs.iter().enumerate() {
                    spawn_input_control(card, module, input, input_index as u8, localizer);
                }
            }
            spawn_inline_diagnostics(card, diagnostic_path, session);
        },
    );
}

fn spawn_input_control(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    input: &InputMetadata,
    input_index: u8,
    localizer: &Localizer,
) {
    let display_name = localized_inspector_input(localizer, input.name, input.display_name, false);
    let description = localized_inspector_input(localizer, input.name, input.description, true);
    let Some(value) = module_parameter(module, input.name) else {
        spawn_inspector_read_only_control(parent, &display_name, "Missing authored value");
        return;
    };
    match (&input.control, value) {
        (InputControl::Curve { .. }, Value::Curve(curve)) => inspector_action_button(
            parent,
            &format!("{}  ·  {} keys  →", display_name, curve.keys.len()),
            CurvesAction::OpenInput(module.id, input_index),
            Some(&description),
        ),
        (InputControl::Gradient, Value::Gradient(gradient)) => inspector_action_button(
            parent,
            &format!("{}  ·  {} color keys  →", display_name, gradient.keys.len()),
            CurvesAction::OpenInput(module.id, input_index),
            Some(&description),
        ),
        (InputControl::Toggle, Value::Bool(value)) => {
            spawn_inspector_toggle_control(
                parent,
                module.id,
                input,
                &display_name,
                &description,
                value,
            );
        }
        (InputControl::Number { .. }, Value::U32(_)) => {
            spawn_inspector_integer_control(parent, module.id, input, &display_name, &description);
        }
        (InputControl::Number { step, min, max }, Value::Scalar(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Scalar,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("", value, 0)],
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec2(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1)],
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec3(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1), ("Z", value[2], 2)],
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec4(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[
                    ("X", value[0], 0),
                    ("Y", value[1], 1),
                    ("Z", value[2], 2),
                    ("W", value[3], 3),
                ],
            );
        }
        (InputControl::Range { step, min, max }, Value::Range(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Range,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("MIN", value.min, 0), ("MAX", value.max, 1)],
            );
        }
        (InputControl::Choice, value) => spawn_inspector_choice_control(
            parent,
            module.id,
            input_index,
            &display_name,
            &description,
            &value,
        ),
        (_, value) => spawn_inspector_read_only_control(
            parent,
            &display_name,
            &format!("{}{}", format_value(value), unit_suffix(input)),
        ),
    }
}

pub(crate) fn localized_inspector_input(
    localizer: &Localizer,
    input: &str,
    fallback: &str,
    description: bool,
) -> String {
    let message = match (input, description) {
        ("spawn_rate", false) => "inspector-input-spawn-rate",
        ("spawn_rate", true) => "inspector-input-spawn-rate-description",
        ("burst_count", false) => "inspector-input-burst-count",
        ("burst_count", true) => "inspector-input-burst-count-description",
        ("shape", false) => "inspector-input-shape",
        ("shape", true) => "inspector-input-shape-description",
        ("lifetime", false) => "inspector-input-lifetime",
        ("lifetime", true) => "inspector-input-lifetime-description",
        ("speed", false) => "inspector-input-speed",
        ("speed", true) => "inspector-input-speed-description",
        ("direction", false) => "inspector-input-direction",
        ("direction", true) => "inspector-input-direction-description",
        ("spread_degrees", false) => "inspector-input-spread",
        ("spread_degrees", true) => "inspector-input-spread-description",
        ("angular_velocity", false) => "inspector-input-angular-velocity",
        ("angular_velocity", true) => "inspector-input-angular-velocity-description",
        ("gravity", false) => "inspector-input-gravity",
        ("gravity", true) => "inspector-input-gravity-description",
        ("drag", false) => "inspector-input-drag",
        ("drag", true) => "inspector-input-drag-description",
        ("turbulence", false) => "inspector-input-turbulence",
        ("turbulence", true) => "inspector-input-turbulence-description",
        ("size", false) => "inspector-input-size-over-life",
        ("size", true) => "inspector-input-size-over-life-description",
        ("opacity", false) => "inspector-input-opacity-over-life",
        ("opacity", true) => "inspector-input-opacity-over-life-description",
        ("color", false) => "inspector-input-color-over-life",
        ("color", true) => "inspector-input-color-over-life-description",
        _ => return fallback.to_owned(),
    };
    localizer.text(message)
}

fn spawn_inspector_integer_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    title: &str,
    description: &str,
) {
    let InputControl::Number { step, min, max } = input.control else {
        return;
    };
    parent
        .spawn((
            EditorTooltip::description(description),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            })
            .with_children(|container| {
                container
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_integer_input())
                    .insert((
                        InspectorNumberControl {
                            module,
                            parameter: input.name,
                            component: 0,
                            kind: InspectorNumberKind::U32,
                            step,
                            min,
                            max,
                        },
                        AccessibleLabel(title.to_owned()),
                    ));
            });
            if let Some(unit) = input.unit {
                row.spawn_empty().apply_scene(label_dim(unit));
            }
        });
}

fn spawn_inspector_number_controls(
    parent: &mut ChildSpawnerCommands,
    input: &InputMetadata,
    title: &str,
    description: &str,
    control: InspectorNumberControl,
    values: &[(&'static str, f32, u8)],
) {
    let bounded_slider = values
        .first()
        .filter(|(axis, _, component)| {
            values.len() == 1
                && axis.is_empty()
                && *component == 0
                && control.kind == InspectorNumberKind::Scalar
        })
        .and_then(|(_, value, _)| {
            control
                .min
                .zip(control.max)
                .and_then(|(min, max)| SliderRowProps::new(*value, min, max, control.step))
        });
    parent
        .spawn((
            EditorTooltip::description(description),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|controls| {
                if let Some(props) = bounded_slider {
                    spawn_slider_input_pair(
                        controls,
                        props,
                        (
                            InspectorSliderControl(control),
                            AccessibleLabel(title.to_owned()),
                        ),
                        (control, AccessibleLabel(title.to_owned())),
                    );
                } else {
                    for (axis, _value, component) in values {
                        let sigil = match *axis {
                            "X" => tokens::TEXT_INPUT_X_AXIS,
                            "Y" => tokens::TEXT_INPUT_Y_AXIS,
                            "Z" => tokens::TEXT_INPUT_Z_AXIS,
                            _ => tokens::TEXT_INPUT_BG,
                        };
                        controls
                            .spawn(Node {
                                flex_grow: 1.0,
                                flex_basis: Val::Px(0.0),
                                min_width: Val::Px(44.0),
                                ..default()
                            })
                            .with_children(|wrapper| {
                                let mut input_entity = wrapper.spawn_empty();
                                if axis.is_empty() {
                                    input_entity.apply_scene(ui_shell::feathers_scalar_input());
                                } else {
                                    input_entity.apply_scene(
                                        ui_shell::feathers_labeled_scalar_input(axis, sigil),
                                    );
                                }
                                input_entity.insert((
                                    InspectorNumberControl {
                                        component: *component,
                                        ..control
                                    },
                                    AccessibleLabel(if axis.is_empty() {
                                        title.to_owned()
                                    } else {
                                        format!("{title} {axis}")
                                    }),
                                ));
                            });
                    }
                }
            });
            if let Some(unit) = input.unit {
                row.spawn_empty().apply_scene(label_dim(unit));
            }
        });
}

fn spawn_inspector_property_label(parent: &mut ChildSpawnerCommands, title: &str) {
    parent.spawn((
        Text::new(title),
        ThemedText,
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        Node {
            width: Val::Percent(36.0),
            min_width: Val::Px(82.0),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

fn spawn_inspector_toggle_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    title: &str,
    description: &str,
    value: bool,
) {
    parent
        .spawn((
            EditorTooltip::description(description),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            let mut checkbox = row.spawn_empty();
            checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                InspectorToggleControl {
                    module,
                    parameter: input.name,
                },
                AccessibleLabel(title.to_owned()),
            ));
            if value {
                checkbox.insert(Checked);
            }
        });
}

fn spawn_inspector_choice_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: u8,
    title: &str,
    description: &str,
    value: &Value,
) {
    let Value::Shape(shape) = value else {
        spawn_inspector_read_only_control(parent, title, &format_value(value.clone()));
        return;
    };
    let current = shape_label(*shape);
    let selected = shape_index(*shape);
    let options = [
        "Point",
        "Circle",
        "Ring",
        "Sphere",
        "Hemisphere",
        "Box",
        "Cylinder",
        "Cone",
    ]
    .into_iter()
    .enumerate()
    .map(|(choice, label)| ComboOption {
        label: label.to_owned(),
        selected: choice == selected,
        action: InspectorAction::SetModuleChoice {
            module,
            input,
            choice: choice as u8,
        },
    })
    .collect::<Vec<_>>();
    spawn_inspector_combo_row(parent, title, current, &options, Some(description));
    match *shape {
        EmitterShape::Point => {}
        EmitterShape::Circle { radius }
        | EmitterShape::Ring { radius }
        | EmitterShape::Sphere { radius }
        | EmitterShape::Hemisphere { radius } => {
            spawn_shape_number_row(
                parent,
                module,
                "Radius",
                "Radius of the spawn shape in local units.",
                &[("", radius, 0)],
            );
        }
        EmitterShape::Box { half_extents } => {
            spawn_shape_number_row(
                parent,
                module,
                "Half Extents",
                "Half-size of the box on each local axis.",
                &[
                    ("X", half_extents[0], 0),
                    ("Y", half_extents[1], 1),
                    ("Z", half_extents[2], 2),
                ],
            );
        }
        EmitterShape::Cylinder { radius, depth } | EmitterShape::Cone { radius, depth } => {
            spawn_shape_number_row(
                parent,
                module,
                "Radius",
                "Radius of the volumetric shape in local units.",
                &[("", radius, 0)],
            );
            spawn_shape_number_row(
                parent,
                module,
                "Depth",
                "Length of the volumetric shape along its local Y axis.",
                &[("", depth, 1)],
            );
        }
    }
}

fn spawn_shape_number_row(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    title: &str,
    description: &str,
    values: &[(&'static str, f32, u8)],
) {
    parent
        .spawn((
            EditorTooltip::description(description),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|controls| {
                for (axis, _value, component) in values {
                    let sigil = match *axis {
                        "X" => tokens::TEXT_INPUT_X_AXIS,
                        "Y" => tokens::TEXT_INPUT_Y_AXIS,
                        "Z" => tokens::TEXT_INPUT_Z_AXIS,
                        _ => tokens::TEXT_INPUT_BG,
                    };
                    controls
                        .spawn(Node {
                            flex_grow: 1.0,
                            flex_basis: Val::Px(0.0),
                            min_width: Val::Px(44.0),
                            ..default()
                        })
                        .with_children(|wrapper| {
                            let mut input = wrapper.spawn_empty();
                            if axis.is_empty() {
                                input.apply_scene(ui_shell::feathers_scalar_input());
                            } else {
                                input.apply_scene(ui_shell::feathers_labeled_scalar_input(
                                    axis, sigil,
                                ));
                            }
                            input.insert((
                                InspectorNumberControl {
                                    module,
                                    parameter: "shape",
                                    component: *component,
                                    kind: InspectorNumberKind::Shape,
                                    step: 0.1,
                                    min: Some(0.1),
                                    max: None,
                                },
                                AccessibleLabel(if axis.is_empty() {
                                    title.to_owned()
                                } else {
                                    format!("{title} {axis}")
                                }),
                            ));
                        });
                }
            });
        });
}

fn spawn_inspector_combo_row(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    current: &str,
    options: &[ComboOption<InspectorAction>],
    description: Option<&str>,
) {
    let mut row = parent.spawn(Node {
        width: Val::Percent(100.0),
        min_height: Val::Px(27.0),
        align_items: AlignItems::Center,
        column_gap: Val::Px(6.0),
        ..default()
    });
    if let Some(description) = description {
        row.insert(EditorTooltip::description(description));
    }
    row.with_children(|row| {
        spawn_inspector_property_label(row, title);
        row.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        spawn_combo_control(row, current, title, options, 150.0);
    });
}

fn spawn_renderer_scalar_control(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    unit: Option<&str>,
    control: RendererNumberControl,
    session: &EditorSession,
) {
    let bounded_slider = renderer_number_input_value(session, control).and_then(|value| {
        let (min, max, step) = match control {
            RendererNumberControl::Uv(_, _) => (0.0, 1.0, 0.01),
            RendererNumberControl::FlipbookFrameRate(_) => (1.0, 120.0, 1.0),
            RendererNumberControl::Softness(_) => return None,
        };
        SliderRowProps::new(value, min, max, step)
    });
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            if let Some(props) = bounded_slider {
                row.spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|controls| {
                    spawn_slider_input_pair(
                        controls,
                        props,
                        (
                            RendererSliderControl(control),
                            AccessibleLabel(title.to_owned()),
                        ),
                        (control, AccessibleLabel(title.to_owned())),
                    );
                });
            } else {
                row.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                row.spawn(Node {
                    width: Val::Px(112.0),
                    ..default()
                })
                .with_children(|input| {
                    input
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((control, AccessibleLabel(title.to_owned())));
                });
            }
            if let Some(unit) = unit {
                row.spawn_empty().apply_scene(label_dim(unit.to_owned()));
            }
        });
}

fn spawn_renderer_toggle_control(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    enabled: bool,
    control: RendererToggleControl,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            let mut checkbox = row.spawn_empty();
            checkbox
                .apply_scene(ui_shell::feathers_checkbox())
                .insert((control, AccessibleLabel(title.to_owned())));
            if enabled {
                checkbox.insert(Checked);
            }
        });
}

fn shape_index(shape: EmitterShape) -> usize {
    match shape {
        EmitterShape::Point => 0,
        EmitterShape::Circle { .. } => 1,
        EmitterShape::Ring { .. } => 2,
        EmitterShape::Sphere { .. } => 3,
        EmitterShape::Hemisphere { .. } => 4,
        EmitterShape::Box { .. } => 5,
        EmitterShape::Cylinder { .. } => 6,
        EmitterShape::Cone { .. } => 7,
    }
}

fn shape_label(shape: EmitterShape) -> &'static str {
    match shape {
        EmitterShape::Point => "Point",
        EmitterShape::Circle { .. } => "Circle",
        EmitterShape::Ring { .. } => "Ring",
        EmitterShape::Sphere { .. } => "Sphere",
        EmitterShape::Hemisphere { .. } => "Hemisphere",
        EmitterShape::Box { .. } => "Box",
        EmitterShape::Cylinder { .. } => "Cylinder",
        EmitterShape::Cone { .. } => "Cone",
    }
}

fn spawn_inspector_read_only_control(parent: &mut ChildSpawnerCommands, title: &str, value: &str) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn_empty().apply_scene(label_dim(value.to_owned()));
        });
}

fn unit_suffix(input: &InputMetadata) -> String {
    input
        .unit
        .map_or_else(String::new, |unit| format!(" {unit}"))
}

fn spawn_renderer_card(
    parent: &mut ChildSpawnerCommands,
    renderer: &aestra_bevy::RendererInstance,
    diagnostic_path: &str,
    session: &EditorSession,
    collapsed: bool,
) {
    let display_name = match renderer.properties {
        RendererProperties::Sprite => "Sprite Renderer",
        RendererProperties::Flipbook { .. } => "Flipbook Renderer",
        _ => "Renderer",
    };
    let base_border = if session.selection.primary == SemanticTarget::Renderer(renderer.id) {
        theme::ACCENT_DIM
    } else {
        theme::BORDER
    };
    spawn_remembered_panel_card(
        parent,
        PanelCardProps::new(display_name, collapsed)
            .with_memory_key(inspector_renderer_key(renderer))
            .with_help("Controls how this emitter is drawn.")
            .with_enabled(renderer.enabled)
            .with_border(base_border),
        InspectorSemanticTarget {
            target: SemanticTarget::Renderer(renderer.id),
            base_border,
        },
        InspectorSelectionTarget(SemanticTarget::Renderer(renderer.id)),
        InspectorAction::ToggleSection(InspectorSection::Renderer(renderer.id)),
        |header| {
            let mut enabled = header.spawn_empty();
            enabled.apply_scene(ui_shell::feathers_checkbox()).insert((
                RendererEnabledControl(renderer.id),
                AccessibleLabel("Enable renderer".into()),
            ));
            if renderer.enabled {
                enabled.insert(Checked);
            }
            spawn_action_menu(
                header,
                "Renderer actions",
                &[
                    ComboOption {
                        label: "Duplicate".into(),
                        selected: false,
                        action: InspectorAction::DuplicateRenderer(renderer.id),
                    },
                    ComboOption {
                        label: "Delete…".into(),
                        selected: false,
                        action: InspectorAction::DeleteRenderer(renderer.id),
                    },
                ],
            );
        },
        |card| {
            let Some(material) = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)
            else {
                spawn_inspector_read_only_control(card, "Material", "Missing");
                spawn_inline_diagnostics(card, diagnostic_path, session);
                return;
            };
            let material_options = session
                .effect
                .materials
                .iter()
                .enumerate()
                .map(|(index, candidate)| ComboOption {
                    label: candidate.name.clone(),
                    selected: candidate.id == material.id,
                    action: InspectorAction::SetRendererMaterial(renderer.id, index),
                })
                .collect::<Vec<_>>();
            spawn_inspector_combo_row(card, "Material", &material.name, &material_options, None);
            let blend_options = [BlendMode::Alpha, BlendMode::Additive, BlendMode::Multiply]
                .into_iter()
                .map(|blend| ComboOption {
                    label: format!("{blend:?}"),
                    selected: blend == material.blend,
                    action: InspectorAction::SetRendererBlend(renderer.id, blend),
                })
                .collect::<Vec<_>>();
            spawn_inspector_combo_row(
                card,
                "Blend",
                &format!("{:?}", material.blend),
                &blend_options,
                None,
            );
            let MaterialProperties::Sprite {
                softness, texture, ..
            } = &material.properties;
            match softness {
                MaterialInput::Constant(_) => spawn_renderer_scalar_control(
                    card,
                    "Softness",
                    None,
                    RendererNumberControl::Softness(renderer.id),
                    session,
                ),
                MaterialInput::Parameter(parameter) => spawn_inspector_read_only_control(
                    card,
                    "Softness",
                    &format!("Parameter {parameter}"),
                ),
            }
            match &renderer.properties {
                RendererProperties::Sprite => {
                    let texture_name = texture
                        .and_then(|id| session.effect.assets.iter().find(|asset| asset.id == id))
                        .map_or("Procedural", |asset| asset.name.as_str());
                    let mut texture_options = vec![ComboOption {
                        label: "Procedural".into(),
                        selected: texture.is_none(),
                        action: InspectorAction::SetRendererTexture(renderer.id, None),
                    }];
                    texture_options.extend(
                        session
                            .effect
                            .assets
                            .iter()
                            .enumerate()
                            .filter(|(_, asset)| asset.kind == aestra_bevy::AssetKind::Texture)
                            .map(|(index, asset)| ComboOption {
                                label: asset.name.clone(),
                                selected: Some(asset.id) == *texture,
                                action: InspectorAction::SetRendererTexture(
                                    renderer.id,
                                    Some(index),
                                ),
                            }),
                    );
                    spawn_inspector_combo_row(
                        card,
                        "Texture",
                        texture_name,
                        &texture_options,
                        None,
                    );
                    if texture.is_some() {
                        for (label, component) in [
                            ("UV Min X", 0),
                            ("UV Min Y", 1),
                            ("UV Max X", 2),
                            ("UV Max Y", 3),
                        ] {
                            spawn_renderer_scalar_control(
                                card,
                                label,
                                None,
                                RendererNumberControl::Uv(renderer.id, component),
                                session,
                            );
                        }
                    }
                }
                RendererProperties::Flipbook {
                    flipbook,
                    time_source,
                    playback,
                    random_start,
                } => {
                    let definition = session
                        .effect
                        .flipbooks
                        .iter()
                        .find(|item| item.id == *flipbook);
                    let flipbook_options = session
                        .effect
                        .flipbooks
                        .iter()
                        .enumerate()
                        .map(|(index, candidate)| ComboOption {
                            label: candidate.name.clone(),
                            selected: candidate.id == *flipbook,
                            action: InspectorAction::SetRendererFlipbook(renderer.id, index),
                        })
                        .collect::<Vec<_>>();
                    spawn_inspector_combo_row(
                        card,
                        "Flipbook",
                        definition.map_or("Missing", |item| item.name.as_str()),
                        &flipbook_options,
                        None,
                    );
                    if let Some(definition) = definition {
                        spawn_renderer_scalar_control(
                            card,
                            "Frame Rate",
                            Some("FPS"),
                            RendererNumberControl::FlipbookFrameRate(renderer.id),
                            session,
                        );
                        spawn_renderer_toggle_control(
                            card,
                            "Looping",
                            definition.looping,
                            RendererToggleControl::FlipbookLooping(renderer.id),
                        );
                    }
                    let time_source_options = [
                        FlipbookTimeSource::ParticleAge,
                        FlipbookTimeSource::EffectTime,
                    ]
                    .into_iter()
                    .map(|candidate| ComboOption {
                        label: format!("{candidate:?}"),
                        selected: candidate == *time_source,
                        action: InspectorAction::SetFlipbookTimeSource(renderer.id, candidate),
                    })
                    .collect::<Vec<_>>();
                    spawn_inspector_combo_row(
                        card,
                        "Time Source",
                        &format!("{time_source:?}"),
                        &time_source_options,
                        None,
                    );
                    let playback_options = [
                        FlipbookPlaybackMode::Forward,
                        FlipbookPlaybackMode::Reverse,
                        FlipbookPlaybackMode::PingPong,
                    ]
                    .into_iter()
                    .map(|candidate| ComboOption {
                        label: format!("{candidate:?}"),
                        selected: candidate == *playback,
                        action: InspectorAction::SetFlipbookPlayback(renderer.id, candidate),
                    })
                    .collect::<Vec<_>>();
                    spawn_inspector_combo_row(
                        card,
                        "Playback",
                        &format!("{playback:?}"),
                        &playback_options,
                        None,
                    );
                    spawn_renderer_toggle_control(
                        card,
                        "Random Start",
                        *random_start,
                        RendererToggleControl::FlipbookRandomStart(renderer.id),
                    );
                }
                _ => {}
            }
            spawn_inline_diagnostics(card, diagnostic_path, session);
        },
    );
}

fn spawn_inline_diagnostics(
    parent: &mut ChildSpawnerCommands,
    path: &str,
    session: &EditorSession,
) {
    for diagnostic in session
        .diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.path.starts_with(path))
    {
        let color = match diagnostic.severity {
            DiagnosticSeverity::Error => Color::srgb(1.0, 0.38, 0.32),
            DiagnosticSeverity::Warning => Color::srgb(1.0, 0.72, 0.28),
            DiagnosticSeverity::Info => theme::TEXT_MUTED,
        };
        parent.spawn((
            Text::new(format!("{:?}: {}", diagnostic.code, diagnostic.message)),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(color),
        ));
    }
}

fn spawn_stage_diagnostics(
    parent: &mut ChildSpawnerCommands,
    stage: StackStage,
    path: &str,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
) {
    for diagnostic in session
        .diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.path == path)
        .filter(|diagnostic| {
            if stage == StackStage::Render {
                return true;
            }
            if diagnostic.code != DiagnosticCode::MissingModule {
                return stage == StackStage::EmitterUpdate;
            }
            registry.0.iter().any(|metadata| {
                metadata.stages.contains(
                    &stage
                        .semantic()
                        .expect("non-render stages have semantic stages"),
                ) && diagnostic.message.contains(&metadata.type_id.0)
            })
        })
    {
        parent.spawn((
            Text::new(format!("⚠ {}", diagnostic.message)),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(Color::srgb(1.0, 0.38, 0.32)),
            Node {
                margin: UiRect::horizontal(Val::Px(10.0)),
                ..default()
            },
        ));
    }
}

fn spawn_module_palette(
    parent: &mut ChildSpawnerCommands,
    registry: &EditorModuleRegistry,
    palette: &ModulePaletteState,
) {
    parent
        .spawn((
            GlobalZIndex(120),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(10.0),
                top: Val::Px(76.0),
                width: Val::Px(360.0),
                max_height: Val::Px(430.0),
                padding: UiRect::all(Val::Px(10.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::ACCENT_DIM),
        ))
        .with_children(|popup| {
            popup
                .spawn(Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|header| {
                    header.spawn((
                        Text::new(format!("ADD TO {}", palette.stage.title())),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    stack_button(header, "×", InspectorAction::CloseModulePalette, 28.0);
                });
            popup.spawn((
                Text::new(format!(
                    "Search: {}▏",
                    if palette.query.is_empty() {
                        "type to filter"
                    } else {
                        &palette.query
                    }
                )),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(32.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL_DARK),
                BorderColor::all(theme::BORDER_BRIGHT),
            ));
            let query = palette.query.to_lowercase();
            let mut results = 0;
            if palette.stage == StackStage::Render
                && (query.is_empty() || "sprite renderer render".contains(&query))
            {
                palette_result(
                    popup,
                    "Sprite Renderer",
                    "Render · translucent particle sprites",
                    InspectorAction::AddSpriteRenderer,
                );
                results += 1;
            }
            if palette.stage == StackStage::Render
                && (query.is_empty() || "flipbook renderer animation render".contains(&query))
            {
                palette_result(
                    popup,
                    "Flipbook Renderer",
                    "Render · animated imported sprite sheet",
                    InspectorAction::AddFlipbookRenderer,
                );
                results += 1;
            }
            for (index, metadata) in registry.0.iter().enumerate() {
                let Some(stage) = palette.stage.semantic() else {
                    continue;
                };
                if !metadata.stages.contains(&stage) || !module_matches(metadata, &query) {
                    continue;
                }
                palette_result(
                    popup,
                    metadata.display_name,
                    &format!("{} · {}", metadata.category, metadata.description),
                    InspectorAction::AddModule(index),
                );
                results += 1;
            }
            if results == 0 {
                popup.spawn((
                    Text::new("No modules match this stage and search."),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                ));
            }
        });
}

fn palette_result(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    subtitle: &str,
    action: InspectorAction,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_plain_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(title.to_owned()),
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: Val::Px(2.0),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                ThemedText,
                Pickable::IGNORE,
            ));
            button.spawn((
                Text::new(subtitle),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                ThemeTextColor(tokens::TEXT_DIM),
                Pickable::IGNORE,
            ));
        });
}

pub(crate) fn stack_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: A,
    width: f32,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
            Node {
                width: Val::Px(width),
                height: Val::Px(21.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
        });
}

fn module_matches(metadata: &ModuleMetadata, query: &str) -> bool {
    query.is_empty()
        || metadata.display_name.to_lowercase().contains(query)
        || metadata.category.to_lowercase().contains(query)
        || metadata.type_id.0.to_lowercase().contains(query)
        || metadata.tags.iter().any(|tag| tag.contains(query))
}

pub(crate) fn module_parameter(module: &ModuleInstance, name: &str) -> Option<Value> {
    match (&module.parameters, name) {
        (
            ModuleParameters::Emission {
                spawn_rate,
                burst_count: _,
            },
            "spawn_rate",
        ) => Some(Value::Scalar(*spawn_rate)),
        (ModuleParameters::Emission { burst_count, .. }, "burst_count") => {
            Some(Value::U32(*burst_count))
        }
        (ModuleParameters::Shape { shape }, "shape") => Some(Value::Shape(*shape)),
        (ModuleParameters::Initialize { lifetime, .. }, "lifetime") => {
            Some(Value::Range(*lifetime))
        }
        (ModuleParameters::Initialize { speed, .. }, "speed") => Some(Value::Range(*speed)),
        (ModuleParameters::Initialize { direction, .. }, "direction") => {
            Some(Value::Vec3(*direction))
        }
        (ModuleParameters::Initialize { spread_degrees, .. }, "spread_degrees") => {
            Some(Value::Scalar(*spread_degrees))
        }
        (
            ModuleParameters::Initialize {
                angular_velocity, ..
            },
            "angular_velocity",
        ) => Some(Value::Range(*angular_velocity)),
        (ModuleParameters::Motion { gravity, .. }, "gravity") => Some(Value::Vec3(*gravity)),
        (ModuleParameters::Motion { drag, .. }, "drag") => Some(Value::Scalar(*drag)),
        (ModuleParameters::Motion { turbulence, .. }, "turbulence") => {
            Some(Value::Scalar(*turbulence))
        }
        (ModuleParameters::Appearance { size, .. }, "size") => Some(Value::Curve(size.clone())),
        (ModuleParameters::Appearance { opacity, .. }, "opacity") => {
            Some(Value::Curve(opacity.clone()))
        }
        (ModuleParameters::Appearance { color, .. }, "color") => {
            Some(Value::Gradient(color.clone()))
        }
        (ModuleParameters::Custom(values), name) => values.get(name).cloned(),
        _ => None,
    }
}

fn select_inspector_header(
    click: On<Pointer<Click>>,
    selectable: Query<&InspectorSelectionTarget>,
    parents: Query<&ChildOf>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let mut entity = click.event_target();
    let target = loop {
        if let Ok(target) = selectable.get(entity) {
            break Some(target.0);
        }
        let Ok(parent) = parents.get(entity) else {
            break None;
        };
        entity = parent.parent();
    };
    let Some(target) = target else {
        return;
    };
    if session.selection.primary != target {
        session.selection.primary = target;
        set_inspector_status(
            &mut session,
            &localizer,
            InspectorStatus::Selected(target.to_string()),
        );
        session.ui_revision += 1;
    }
}

#[derive(Component)]
pub(crate) struct InspectorTitle;

#[derive(Component)]
struct InspectorSemanticTarget {
    target: SemanticTarget,
    base_border: Color,
}

#[derive(Component, Debug, Clone, Copy)]
struct InspectorSelectionTarget(SemanticTarget);

#[derive(Resource, Default)]
pub(crate) struct InspectorFocus {
    pub(crate) target: Option<SemanticTarget>,
    pub(crate) wait_frames: u8,
    pub(crate) highlight: Option<SemanticTarget>,
    pub(crate) highlight_remaining: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectorNumberKind {
    U32,
    Scalar,
    Vector,
    Range,
    Shape,
}

#[derive(Component, Debug, Clone, Copy)]
struct InspectorNumberControl {
    module: ModuleId,
    parameter: &'static str,
    component: u8,
    kind: InspectorNumberKind,
    step: f32,
    min: Option<f32>,
    max: Option<f32>,
}

#[derive(Component, Debug, Clone, Copy)]
struct InspectorSliderControl(InspectorNumberControl);

#[derive(Component, Debug, Clone, Copy)]
enum EmitterNumberControl {
    Start,
    Duration,
    Translation(u8),
    Rotation(u8),
    Scale(u8),
}

#[derive(Component, Debug, Clone, Copy)]
struct EffectClipNumberControl {
    clip: EffectClipId,
    control: EmitterNumberControl,
}

#[derive(Component, Debug, Clone, Copy)]
enum DocumentTextControl {
    EffectName,
    EmitterName,
}

#[derive(Component, Debug, Clone, Copy)]
enum DocumentToggleControl {
    EmitterEnabled,
}

#[derive(Component, Debug, Clone, Copy)]
struct EmitterCapacityControl;

#[derive(Component)]
struct NumericScrubInput;

#[derive(Debug, Clone, Copy)]
enum NumericScrubTarget {
    Inspector(InspectorNumberControl),
    Emitter(EmitterNumberControl),
    EffectClip(EffectClipNumberControl),
    Renderer(RendererNumberControl),
}

#[derive(Debug, Clone, Copy)]
struct ActiveNumericScrub {
    entity: Entity,
    target: NumericScrubTarget,
    initial: f32,
    raw: f32,
    current: f32,
}

#[derive(Resource, Default)]
struct NumericScrubState {
    active: Option<ActiveNumericScrub>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveBoundedSlider {
    entity: Entity,
    target: NumericScrubTarget,
    initial: f32,
}

#[derive(Resource, Default)]
struct BoundedSliderState {
    active: Option<ActiveBoundedSlider>,
}

#[derive(Component, Debug, Clone, Copy)]
struct InspectorToggleControl {
    module: ModuleId,
    parameter: &'static str,
}

#[derive(Component, Debug, Clone, Copy)]
struct ModuleEnabledControl(ModuleId);

#[derive(Component, Debug, Clone, Copy)]
struct RendererEnabledControl(RendererId);

#[derive(Component, Debug, Clone, Copy)]
enum RendererNumberControl {
    Softness(RendererId),
    Uv(RendererId, u8),
    FlipbookFrameRate(RendererId),
}

#[derive(Component, Debug, Clone, Copy)]
struct RendererSliderControl(RendererNumberControl);

#[derive(Component, Debug, Clone, Copy)]
enum RendererToggleControl {
    FlipbookLooping(RendererId),
    FlipbookRandomStart(RendererId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackStage {
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
    Render,
}

impl StackStage {
    const ALL: [Self; 4] = [
        Self::EmitterUpdate,
        Self::ParticleSpawn,
        Self::ParticleUpdate,
        Self::Render,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::EmitterUpdate => "EMITTER UPDATE",
            Self::ParticleSpawn => "PARTICLE SPAWN",
            Self::ParticleUpdate => "PARTICLE UPDATE",
            Self::Render => "RENDER",
        }
    }

    fn semantic(self) -> Option<StageKind> {
        match self {
            Self::EmitterUpdate => Some(StageKind::EmitterUpdate),
            Self::ParticleSpawn => Some(StageKind::ParticleSpawn),
            Self::ParticleUpdate => Some(StageKind::ParticleUpdate),
            Self::Render => None,
        }
    }
}

#[derive(Resource)]
pub(crate) struct EditorModuleRegistry(pub(crate) ModuleRegistry);

impl Default for EditorModuleRegistry {
    fn default() -> Self {
        Self(ModuleRegistry::builtin())
    }
}

#[derive(Resource)]
pub(crate) struct ModulePaletteState {
    pub(crate) open: bool,
    pub(crate) stage: StackStage,
    pub(crate) query: String,
}

impl Default for ModulePaletteState {
    fn default() -> Self {
        Self {
            open: false,
            stage: StackStage::EmitterUpdate,
            query: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InspectorSection {
    Module(ModuleId),
    Renderer(RendererId),
}
