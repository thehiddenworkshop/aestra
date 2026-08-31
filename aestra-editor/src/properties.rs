//! Properties ownership: module-stack UI, semantic property editing, numeric scrubbing,
//! navigation focus, and contextual help.

use crate::feathers::breadcrumb::{BreadcrumbItem, BreadcrumbProps, spawn_breadcrumb};
use crate::feathers::panel_card::{
    PanelCardProps, RememberedPanelCard, spawn_panel_card as spawn_remembered_panel_card,
};
use crate::feathers::slider_row::{SliderNumberInputPair, SliderRowProps, spawn_slider_input_pair};
use crate::timeline::{
    EffectClipChildSelection, EffectClipPath, TimelineState, resolve_effect_clip_path,
};
use crate::*;
use aestra_bevy::{
    ChoreographyEventId, ChoreographyEventKind, ChoreographyEventPayload, ColorKey, Curve,
    CurveKey, EffectAsset, EffectClip, EffectClipId, EffectParameter, Gradient, MarkerId,
    MarkerTimeReference, ParameterId, ScalarRange, ValueType,
};
use aestra_compiler::{
    InputControl, InputEvaluationDomain, InputMetadata, InputSourceKind, ModuleRegistry,
};
use bevy::{
    feathers::controls::ButtonVariant,
    ui::InteractionDisabled,
    ui::Selected,
    ui_widgets::{Activate, SliderValue},
};
use fluent_bundle::FluentArgs;

pub(crate) const PROPERTIES_HIGHLIGHT_DURATION: f32 = 1.6;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PropertiesSet {
    Input,
    Actions,
    Sync,
}

pub(crate) struct PropertiesPlugin;

impl Plugin for PropertiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorModuleRegistry>()
            .init_resource::<ModulePaletteState>()
            .init_resource::<EffectClipRepairState>()
            .init_resource::<PropertiesFocus>()
            .init_resource::<NumericScrubState>()
            .init_resource::<BoundedSliderState>()
            .add_observer(queue_properties_action_activation)
            .add_observer(handle_document_text_change)
            .add_observer(handle_document_toggle_change)
            .add_observer(handle_emitter_capacity_change)
            .add_observer(handle_properties_toggle_change)
            .add_observer(handle_module_enabled_change)
            .add_observer(handle_renderer_enabled_change)
            .add_observer(handle_renderer_scalar_change)
            .add_observer(handle_renderer_toggle_change)
            .add_observer(handle_emitter_scalar_change)
            .add_observer(handle_effect_clip_scalar_change)
            .add_observer(handle_marker_scalar_change)
            .add_observer(handle_choreography_event_scalar_change)
            .add_observer(handle_choreography_event_payload_text_change)
            .add_observer(handle_start_reference_offset_change)
            .add_observer(handle_effect_clip_parameter_integer_change)
            .add_observer(handle_effect_clip_parameter_scalar_change)
            .add_observer(handle_effect_clip_parameter_text_change)
            .add_observer(handle_effect_clip_parameter_toggle_change)
            .add_observer(update_effect_clip_repair_query)
            .add_observer(handle_properties_integer_change)
            .add_observer(handle_properties_scalar_change)
            .add_observer(handle_bounded_slider_change)
            .add_observer(begin_numeric_scrub)
            .add_observer(update_numeric_scrub)
            .add_observer(finish_numeric_scrub)
            .add_observer(select_properties_header)
            .add_systems(Update, module_palette_keyboard.in_set(PropertiesSet::Input))
            .add_systems(
                Update,
                handle_properties_actions.in_set(PropertiesSet::Actions),
            )
            .add_systems(
                Update,
                (
                    (
                        sync_emitter_capacity_inputs,
                        sync_emitter_number_inputs,
                        sync_effect_clip_number_inputs,
                        sync_marker_number_inputs,
                        sync_choreography_event_number_inputs,
                        sync_start_reference_offset_inputs,
                        sync_effect_clip_parameter_number_inputs,
                        sync_properties_number_inputs,
                        sync_properties_slider_inputs,
                        sync_renderer_number_inputs,
                        sync_renderer_slider_inputs,
                        sync_effect_clip_properties_timing,
                        sync_effect_clip_repair_candidates,
                    )
                        .chain(),
                    scroll_properties_to_focus,
                    update_properties_highlight,
                    decorate_numeric_scrub_inputs,
                    sync_module_input_public_toggle_visibility,
                )
                    .chain()
                    .in_set(PropertiesSet::Sync),
            );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartReferenceTarget {
    Emitter(EmitterId),
    EffectClip(EffectClipId),
    ChoreographyEvent(ChoreographyEventId),
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) enum PropertiesAction {
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
    ToggleSection(PropertiesSection),
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
    DeleteMarker(MarkerId),
    DeleteChoreographyEvent(ChoreographyEventId),
    SetChoreographyEventKind {
        id: ChoreographyEventId,
        kind: ChoreographyEventKind,
    },
    SetStartReference {
        target: StartReferenceTarget,
        marker: Option<MarkerId>,
    },
    RepairEffectClipSource {
        clip: EffectClipId,
        source: EffectAssetRef,
    },
    ResetEffectClipParameter {
        clip: EffectClipId,
        parameter: ParameterId,
    },
    ToggleModuleInputPublic {
        module: ModuleId,
        input: u8,
    },
    SetModuleInputSource {
        module: ModuleId,
        input: u8,
        source: PropertySourceKind,
    },
}

pub(crate) type PropertySourceKind = InputSourceKind;

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
enum PropertiesStatus {
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

fn set_properties_status(
    session: &mut EditorSession,
    localizer: &Localizer,
    status: PropertiesStatus,
) {
    session.status = localize_properties_status(status, localizer);
}

fn localize_properties_status(status: PropertiesStatus, localizer: &Localizer) -> String {
    let (message_id, argument) = match status {
        PropertiesStatus::SelectedCompiled(target) => {
            ("properties-status-selected-compiled", ("target", target))
        }
        PropertiesStatus::Selected(target) => ("properties-status-selected", ("target", target)),
        PropertiesStatus::ModuleRegistryUnavailable => {
            return localizer.text("properties-status-module-registry-unavailable");
        }
        PropertiesStatus::ModuleMissing => {
            return localizer.text("properties-status-module-missing");
        }
        PropertiesStatus::InputMetadataUnavailable => {
            return localizer.text("properties-status-input-metadata-unavailable");
        }
        PropertiesStatus::NotChoice(input) => ("properties-status-not-choice", ("input", input)),
        PropertiesStatus::ChoiceUnavailable => {
            return localizer.text("properties-status-choice-unavailable");
        }
        PropertiesStatus::TargetUnavailable => {
            return localizer.text("properties-status-target-unavailable");
        }
        PropertiesStatus::FiniteNumberRequired(parameter) => (
            "properties-status-finite-number-required",
            ("parameter", parameter),
        ),
        PropertiesStatus::IncompatibleMetadata(parameter) => (
            "properties-status-incompatible-metadata",
            ("parameter", parameter),
        ),
        PropertiesStatus::Updated(target) => ("properties-status-updated", ("target", target)),
        PropertiesStatus::NameRequired(target) => {
            ("properties-status-name-required", ("target", target))
        }
        PropertiesStatus::EventAdded { trigger, target } => {
            let mut args = FluentArgs::new();
            args.set("trigger", trigger);
            args.set("target", target);
            return localizer.text_with("properties-status-event-added", &args);
        }
        PropertiesStatus::EventRemoved => {
            return localizer.text("properties-status-event-removed");
        }
        PropertiesStatus::EventDuplicate => {
            return localizer.text("properties-status-event-duplicate");
        }
        PropertiesStatus::EventSelfTarget => {
            return localizer.text("properties-status-event-self-target");
        }
        PropertiesStatus::EventTargetMissing => {
            return localizer.text("properties-status-event-target-missing");
        }
        PropertiesStatus::RepairRejected(reason) => {
            let mut args = FluentArgs::new();
            args.set("reason", reason);
            return localizer.text_with("properties-repair-rejected", &args);
        }
    };
    let mut args = FluentArgs::new();
    args.set(argument.0, argument.1);
    localizer.text_with(message_id, &args)
}

fn queue_properties_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<PropertiesAction>, With<FeathersActionButton>)>,
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
fn handle_properties_actions(
    mut commands: Commands,
    mut actions: Query<
        (
            Entity,
            &Interaction,
            &PropertiesAction,
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
                    PropertiesAction::OpenModulePalette(stage) => {
                        palette.open = true;
                        palette.stage = stage;
                        palette.query.clear();
                        session.ui_revision += 1;
                    }
                    PropertiesAction::CloseModulePalette => {
                        palette.open = false;
                        session.ui_revision += 1;
                    }
                    PropertiesAction::AddModule(index) => {
                        let module = registry
                            .0
                            .iter()
                            .nth(index)
                            .and_then(|metadata| registry.0.instantiate(&metadata.type_id));
                        if let Some(module) = module {
                            session.add_module(module);
                            palette.open = false;
                        } else {
                            set_properties_status(
                                &mut session,
                                &localizer,
                                PropertiesStatus::ModuleRegistryUnavailable,
                            );
                        }
                    }
                    PropertiesAction::AddSpriteRenderer => {
                        session.add_sprite_renderer();
                        palette.open = false;
                    }
                    PropertiesAction::AddFlipbookRenderer => {
                        session.add_flipbook_renderer();
                        palette.open = false;
                    }
                    PropertiesAction::SetModuleChoice {
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
                    PropertiesAction::MoveModule(id, direction) => {
                        session.move_module(id, direction);
                    }
                    PropertiesAction::DuplicateModule(id) => session.duplicate_module(id),
                    PropertiesAction::DeleteModule(id) => {
                        if preview_module_deletion(&mut session, id) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                            workspace.clear();
                        }
                    }
                    PropertiesAction::SetRendererMaterial(id, index) => {
                        if let Some(material) = session
                            .effect
                            .materials
                            .get(index)
                            .map(|material| material.id)
                        {
                            session.set_renderer_material(id, material);
                        }
                    }
                    PropertiesAction::SetRendererBlend(id, blend) => {
                        session.set_renderer_blend(id, blend);
                    }
                    PropertiesAction::SetRendererTexture(id, index) => {
                        let texture = index
                            .and_then(|index| session.effect.assets.get(index))
                            .filter(|asset| asset.kind == aestra_bevy::AssetKind::Texture)
                            .map(|asset| asset.id);
                        session.set_renderer_texture(id, texture);
                    }
                    PropertiesAction::SetRendererFlipbook(id, index) => {
                        if let Some(flipbook) = session
                            .effect
                            .flipbooks
                            .get(index)
                            .map(|flipbook| flipbook.id)
                        {
                            session.set_renderer_flipbook(id, flipbook);
                        }
                    }
                    PropertiesAction::SetFlipbookTimeSource(id, value) => {
                        session.set_flipbook_time_source(id, value);
                    }
                    PropertiesAction::SetFlipbookPlayback(id, value) => {
                        session.set_flipbook_playback(id, value);
                    }
                    PropertiesAction::AddEventLink { trigger, target } => {
                        let target_name = session
                            .effect
                            .emitters
                            .iter()
                            .find(|emitter| emitter.id == target)
                            .map(|emitter| emitter.name.clone());
                        let result = session.add_event_link(trigger, target);
                        let status = match result {
                            Ok(_) => PropertiesStatus::EventAdded {
                                trigger: localized_event_trigger(&localizer, trigger),
                                target: target_name.unwrap_or_else(|| target.to_string()),
                            },
                            Err(crate::session::EventLinkError::SameEmitter) => {
                                PropertiesStatus::EventSelfTarget
                            }
                            Err(crate::session::EventLinkError::Duplicate) => {
                                PropertiesStatus::EventDuplicate
                            }
                            Err(crate::session::EventLinkError::TargetMissing) => {
                                PropertiesStatus::EventTargetMissing
                            }
                        };
                        set_properties_status(&mut session, &localizer, status);
                    }
                    PropertiesAction::DeleteEventLink(id) => {
                        if session.remove_event_link(id) {
                            set_properties_status(
                                &mut session,
                                &localizer,
                                PropertiesStatus::EventRemoved,
                            );
                        } else {
                            set_properties_status(
                                &mut session,
                                &localizer,
                                PropertiesStatus::TargetUnavailable,
                            );
                        }
                    }
                    PropertiesAction::DeleteMarker(id) => {
                        session.execute(
                            "Deleted timeline marker",
                            EffectCommand::RemoveMarker { id },
                            true,
                        );
                    }
                    PropertiesAction::DeleteChoreographyEvent(id) => {
                        session.execute(
                            "Deleted choreography event",
                            EffectCommand::RemoveChoreographyEvent { id },
                            true,
                        );
                    }
                    PropertiesAction::SetChoreographyEventKind { id, kind } => {
                        let payload = match kind {
                            ChoreographyEventKind::GameplayNotify => {
                                ChoreographyEventPayload::GameplayNotify {
                                    topic: String::new(),
                                }
                            }
                            ChoreographyEventKind::PlaySound => {
                                ChoreographyEventPayload::PlaySound { cue: String::new() }
                            }
                            ChoreographyEventKind::CameraShake => {
                                ChoreographyEventPayload::CameraShake { intensity: 1.0 }
                            }
                            ChoreographyEventKind::SpawnChildEffect => {
                                ChoreographyEventPayload::SpawnChildEffect {
                                    effect: String::new(),
                                }
                            }
                        };
                        session.execute(
                            "Changed choreography event type",
                            EffectCommand::SetChoreographyEventPayload { id, payload },
                            true,
                        );
                    }
                    PropertiesAction::SetStartReference { target, marker } => {
                        set_start_reference(&mut session, target, marker, &localizer);
                    }
                    PropertiesAction::RepairEffectClipSource { clip, source } => {
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
                                    localizer.text("properties-repair-effect-clip-command"),
                                    EffectCommand::SetEffectClipSource { id: clip, source },
                                    true,
                                );
                                if let Some(repair) = repair.as_deref_mut() {
                                    repair.query.clear();
                                }
                            }
                            Err(reason) => set_properties_status(
                                &mut session,
                                &localizer,
                                PropertiesStatus::RepairRejected(reason),
                            ),
                        }
                    }
                    PropertiesAction::ResetEffectClipParameter { clip, parameter } => {
                        session.execute(
                            localizer.text("properties-reset-effect-clip-parameter-command"),
                            EffectCommand::RemoveEffectClipParameterOverride {
                                id: clip,
                                parameter,
                            },
                            true,
                        );
                    }
                    PropertiesAction::ToggleModuleInputPublic { module, input } => {
                        toggle_module_input_public(
                            &mut session,
                            &registry.0,
                            module,
                            input,
                            &localizer,
                        );
                    }
                    PropertiesAction::SetModuleInputSource {
                        module,
                        input,
                        source,
                    } => {
                        set_module_input_source(
                            &mut session,
                            &registry.0,
                            module,
                            input,
                            source,
                            &localizer,
                        );
                    }
                    PropertiesAction::DuplicateRenderer(id) => session.duplicate_renderer(id),
                    PropertiesAction::DeleteRenderer(id) => {
                        if preview_renderer_deletion(&mut session, id) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                            workspace.clear();
                        }
                    }
                    PropertiesAction::ToggleSection(section) => {
                        if toggle_persisted_properties_section(&session, &mut settings, section) {
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

fn unique_effect_parameter_name_from_base(effect: &EffectAsset, base: &str) -> String {
    if !effect
        .parameters
        .iter()
        .any(|parameter| parameter.name == base)
    {
        return base.to_owned();
    }
    (2..)
        .map(|index| format!("{base} {index}"))
        .find(|name| {
            !effect
                .parameters
                .iter()
                .any(|parameter| &parameter.name == name)
        })
        .expect("the unbounded numeric suffix always yields a unique parameter name")
}

fn toggle_module_input_public(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module_id: ModuleId,
    input_index: u8,
    localizer: &Localizer,
) -> bool {
    let Some((_, input_name)) =
        properties_module_input_target(session, registry, module_id, input_index)
    else {
        return false;
    };
    let binding = session
        .effect
        .emitters
        .iter()
        .flat_map(|emitter| emitter.modules.iter())
        .find(|module| module.id == module_id)
        .and_then(|module| module.bindings.get(input_name))
        .copied();
    let Some(parameter_id) = binding else {
        return expose_module_input(session, registry, module_id, input_index, localizer);
    };
    update_effect_parameter(session, localizer, parameter_id, |parameter| {
        parameter.exposed = !parameter.exposed;
    })
}

fn expose_module_input(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module_id: ModuleId,
    input_index: u8,
    localizer: &Localizer,
) -> bool {
    let Some((emitter, input_name)) =
        properties_module_input_target(session, registry, module_id, input_index)
    else {
        return false;
    };
    let Some(module) = session
        .effect
        .emitters
        .iter()
        .find(|candidate| candidate.id == emitter)
        .and_then(|emitter| {
            emitter
                .modules
                .iter()
                .find(|candidate| candidate.id == module_id)
        })
    else {
        return false;
    };
    if module.bindings.contains_key(input_name) {
        return false;
    }
    let Some(mut default) = module_parameter(module, input_name) else {
        return false;
    };
    default.regenerate_ids();
    let metadata = registry.get(&module.module_type);
    let display_name = metadata
        .and_then(|metadata| metadata.inputs.get(input_index as usize))
        .map_or_else(
            || input_name.to_owned(),
            |input| localized_properties_input(localizer, input.name, input.display_name, false),
        );
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: unique_effect_parameter_name_from_base(&session.effect, &display_name),
        default,
        exposed: true,
    };
    let parameter_id = parameter.id;
    let parameter_index = session.effect.parameters.len();
    session.execute_transaction(
        EffectTransaction::new(
            localizer.text("properties-expose-module-input-command"),
            vec![
                EffectCommand::AddParameter {
                    parameter,
                    index: parameter_index,
                },
                EffectCommand::BindModuleParameter {
                    emitter,
                    module: module_id,
                    parameter: input_name.to_owned(),
                    source: parameter_id,
                },
            ],
        ),
        true,
    )
}

fn properties_module_input_target<'a>(
    session: &EditorSession,
    registry: &'a ModuleRegistry,
    module: ModuleId,
    input: u8,
) -> Option<(EmitterId, &'a str)> {
    let (emitter, module) = session.effect.emitters.iter().find_map(|emitter| {
        emitter
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .map(|module| (emitter.id, module))
    })?;
    let metadata = registry.get(&module.module_type)?;
    metadata
        .inputs
        .get(input as usize)
        .map(|input| (emitter, input.name))
}

fn set_module_input_source(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module: ModuleId,
    input_index: u8,
    source: PropertySourceKind,
    localizer: &Localizer,
) -> bool {
    let Some((emitter, parameter)) =
        properties_module_input_target(session, registry, module, input_index)
    else {
        return false;
    };
    let Some((module_instance, input)) = session
        .effect
        .emitters
        .iter()
        .flat_map(|emitter| emitter.modules.iter())
        .find(|candidate| candidate.id == module)
        .and_then(|module| {
            registry
                .get(&module.module_type)
                .and_then(|metadata| metadata.inputs.get(input_index as usize))
                .map(|input| (module, input))
        })
    else {
        return false;
    };
    if !input.sources.contains(&source) {
        return false;
    }
    if module_instance.property_source(parameter) == Some(source) {
        return false;
    }
    let Some(current) = properties_module_parameter(session, module, parameter) else {
        return false;
    };
    let mut commands = Vec::with_capacity(4);
    let Some(active_source) = module_instance.property_source(parameter) else {
        return false;
    };
    let active_has_stored_value = module_instance
        .property_source_values
        .get(parameter)
        .is_some_and(|values| {
            values
                .iter()
                .any(|candidate| candidate.source == active_source)
        });
    if active_source != PropertySourceKind::Constant
        && (module_instance.bindings.contains_key(parameter) || !active_has_stored_value)
    {
        commands.push(EffectCommand::SetModulePropertySourceValue {
            emitter,
            module,
            parameter: parameter.to_owned(),
            source: active_source,
            value: current.clone(),
        });
    }
    let target_value = if source == PropertySourceKind::Constant {
        module_instance.parameter_value(parameter)
    } else {
        module_instance
            .property_value_for_source(parameter, source)
            .or_else(|| initial_property_source_value(input, &current, source))
    };
    if source != PropertySourceKind::Constant
        && module_instance
            .property_value_for_source(parameter, source)
            .is_none()
    {
        let Some(value) = target_value.clone() else {
            return false;
        };
        commands.push(EffectCommand::SetModulePropertySourceValue {
            emitter,
            module,
            parameter: parameter.to_owned(),
            source,
            value,
        });
    }
    if let Some(parameter_id) = module_instance.bindings.get(parameter) {
        let Some(mut effect_parameter) = session
            .effect
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter_id)
            .cloned()
        else {
            return false;
        };
        let Some(value) = target_value else {
            return false;
        };
        effect_parameter.default = detached_property_value(value);
        commands.push(EffectCommand::SetParameter {
            id: *parameter_id,
            parameter: effect_parameter,
        });
    } else if source == PropertySourceKind::Constant
        && active_source != PropertySourceKind::Constant
        && !active_has_stored_value
    {
        let Some(value) = target_value else {
            return false;
        };
        commands.push(EffectCommand::SetModuleParameter {
            emitter,
            module,
            parameter: parameter.to_owned(),
            value: detached_property_value(value),
        });
    }
    commands.push(EffectCommand::SetModulePropertySource {
        emitter,
        module,
        parameter: parameter.to_owned(),
        source,
    });
    session.execute_transaction(
        EffectTransaction::new(localizer.text("properties-change-source-command"), commands),
        true,
    )
}

fn detached_property_value(mut value: Value) -> Value {
    value.regenerate_ids();
    value
}

fn initial_property_source_value(
    input: &InputMetadata,
    current: &Value,
    source: PropertySourceKind,
) -> Option<Value> {
    let scalar = match current {
        Value::Scalar(value) => Some(*value),
        Value::Range(range) => Some((range.min + range.max) * 0.5),
        Value::Curve(curve) => Some(curve.sample(0.0)),
        _ => None,
    };
    match source {
        PropertySourceKind::RandomRange => {
            let value = scalar?;
            let (step, min, max) = numeric_source_limits(&input.control)?;
            let low = min.map_or(value - step, |minimum| (value - step).max(minimum));
            let high = max.map_or(value + step, |maximum| (value + step).min(maximum));
            Some(Value::Range(ScalarRange::new(low.min(high), high.max(low))))
        }
        PropertySourceKind::Curve(_) => {
            let value = scalar?;
            Some(Value::Curve(Curve::new(vec![
                CurveKey::new(0.0, value),
                CurveKey::new(1.0, value),
            ])))
        }
        PropertySourceKind::Gradient(_) => {
            let Value::Gradient(gradient) = current else {
                return None;
            };
            let color = gradient.sample(0.0);
            Some(Value::Gradient(Gradient::new(vec![
                ColorKey::new(0.0, color),
                ColorKey::new(1.0, color),
            ])))
        }
        PropertySourceKind::Constant => None,
    }
}

fn numeric_source_limits(control: &InputControl) -> Option<(f32, Option<f32>, Option<f32>)> {
    match control {
        InputControl::Number { step, min, max } | InputControl::Range { step, min, max } => {
            Some((*step, *min, *max))
        }
        InputControl::Curve { step, min, max } => Some((*step, Some(*min), Some(*max))),
        _ => None,
    }
}

fn properties_curve_limits(input: &InputMetadata, curve: &Curve) -> Option<(f32, f32, f32)> {
    let (step, min, max) = numeric_source_limits(&input.control)?;
    let authored_min = curve
        .keys
        .iter()
        .map(|key| key.value)
        .fold(f32::INFINITY, f32::min);
    let authored_max = curve
        .keys
        .iter()
        .map(|key| key.value)
        .fold(f32::NEG_INFINITY, f32::max);
    let minimum = min.unwrap_or(if authored_min.is_finite() {
        authored_min
    } else {
        0.0
    });
    let maximum = max.unwrap_or_else(|| {
        if authored_max.is_finite() {
            authored_max.max(minimum + step)
        } else {
            minimum + step
        }
    });
    Some((step, minimum, maximum.max(minimum + f32::EPSILON)))
}

// Properties domain implementation.
fn semantic_target_exists(effect: &EffectAsset, target: SemanticTarget) -> bool {
    match target {
        SemanticTarget::Effect(id) => effect.id == id,
        SemanticTarget::EffectClip(id) => effect.effect_clips.iter().any(|clip| clip.id == id),
        SemanticTarget::Marker(id) => effect.markers.iter().any(|marker| marker.id == id),
        SemanticTarget::ChoreographyEvent(id) => effect
            .choreography_events
            .iter()
            .any(|event| event.id == id),
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
    focus: &mut PropertiesFocus,
    target: SemanticTarget,
    localizer: &Localizer,
) -> bool {
    if !semantic_target_exists(&session.effect, target) {
        return false;
    }
    if matches!(
        target,
        SemanticTarget::Emitter(_)
            | SemanticTarget::Module(_)
            | SemanticTarget::Renderer(_)
            | SemanticTarget::ChoreographyEvent(_)
    ) {
        session.selection.primary = target;
    }
    focus.target = Some(target);
    focus.wait_frames = 2;
    focus.highlight = Some(target);
    focus.highlight_remaining = PROPERTIES_HIGHLIGHT_DURATION;
    set_properties_status(
        session,
        localizer,
        PropertiesStatus::SelectedCompiled(target.to_string()),
    );
    session.ui_revision += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EFFECT_PATH, EFFECT_SOURCE};
    use aestra_bevy::{EffectClipSeed, EffectMarker};

    fn test_localizer() -> Localizer {
        Localizer::new("en-US").unwrap()
    }

    fn clear_effect_parameters_and_bindings(session: &mut EditorSession) {
        session.effect.parameters.clear();
        for emitter in &mut session.effect.emitters {
            for module in &mut emitter.modules {
                module.bindings.clear();
            }
        }
    }

    #[test]
    fn effect_clip_parameter_entries_use_defaults_and_report_stale_overrides() {
        let exposed = ParameterId::new();
        let hidden = ParameterId::new();
        let missing = ParameterId::new();
        let mut source = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        source.parameters = vec![
            aestra_bevy::EffectParameter {
                id: exposed,
                name: "Intensity".into(),
                default: Value::Scalar(2.0),
                exposed: true,
            },
            aestra_bevy::EffectParameter {
                id: hidden,
                name: "Internal".into(),
                default: Value::Bool(false),
                exposed: false,
            },
        ];
        let mut clip = EffectClip::new(EffectAssetRef::new(source.id), 0.0, 1.0);
        clip.parameter_overrides.insert(hidden, Value::Bool(true));
        clip.parameter_overrides.insert(missing, Value::U32(4));

        let entries = effect_clip_parameter_entries(&clip, Some(&source));

        assert_eq!(entries[0].id, exposed);
        assert_eq!(entries[0].value, Value::Scalar(2.0));
        assert!(!entries[0].overridden);
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.id == hidden)
                .unwrap()
                .issue,
            Some(EffectClipParameterIssue::Hidden)
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.id == missing)
                .unwrap()
                .issue,
            Some(EffectClipParameterIssue::Missing)
        );
    }

    #[test]
    fn effect_clip_parameter_edit_creates_an_undoable_override() {
        let parameter = ParameterId::new();
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let source = EffectAssetRef::new(aestra_bevy::EffectId::from_u128(0xe11ec7));
        let clip = EffectClip::new(source, 0.0, 1.0);
        let clip_id = clip.id;
        session.effect.effect_clips.push(clip);
        set_effect_clip_parameter_override(
            &mut session,
            &test_localizer(),
            clip_id,
            parameter,
            Value::Scalar(3.5),
        );
        let authored = session
            .effect
            .effect_clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .unwrap();
        assert_eq!(authored.parameter_overrides[&parameter], Value::Scalar(3.5));
        session.undo();
        assert!(
            session
                .effect
                .effect_clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .unwrap()
                .parameter_overrides
                .is_empty()
        );
    }

    #[test]
    fn effect_clip_parameter_scrub_previews_and_commits_one_ordered_override() {
        let parameter = ParameterId::new();
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let source = EffectAssetRef::new(aestra_bevy::EffectId::from_u128(0xe11ec7));
        let clip = EffectClip::new(source, 0.0, 1.0);
        let clip_id = clip.id;
        session.effect.effect_clips.push(clip);
        let target = NumericScrubTarget::EffectClipParameter(EffectClipParameterScrubControl {
            clip: clip_id,
            parameter,
            component: 0,
            values: [0.8, 1.5, 0.0, 0.0],
            kind: EffectClipParameterScrubKind::Range,
        });

        assert_eq!(numeric_scrub_step(target), 0.1);
        assert_eq!(normalize_numeric_scrub_value(&session, target, 2.0), 1.5);
        assert!(preview_numeric_scrub(&mut session, target, 1.2));
        assert!(
            session
                .effect
                .effect_clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .unwrap()
                .parameter_overrides
                .is_empty()
        );

        commit_numeric_scrub(&mut session, target, 1.2, &test_localizer());
        assert_eq!(
            session
                .effect
                .effect_clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .unwrap()
                .parameter_overrides
                .get(&parameter),
            Some(&Value::Range(aestra_bevy::ScalarRange {
                min: 1.2,
                max: 1.5
            })),
            "{}",
            session.status
        );
        session.undo();
        assert!(
            session
                .effect
                .effect_clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .unwrap()
                .parameter_overrides
                .is_empty()
        );
    }

    #[test]
    fn effect_parameter_editor_updates_definition_and_undoes_as_one_edit() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let parameter = EffectParameter {
            id: ParameterId::new(),
            name: "Intensity".into(),
            default: Value::Scalar(1.0),
            exposed: true,
        };
        let id = parameter.id;
        session.effect.parameters.push(parameter);

        assert!(update_effect_parameter(
            &mut session,
            &test_localizer(),
            id,
            |parameter| {
                parameter.name = "Power".into();
                parameter.default = Value::Scalar(2.0);
                parameter.exposed = false;
            }
        ));
        let edited = session
            .effect
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .unwrap();
        assert_eq!(edited.name, "Power");
        assert_eq!(edited.default, Value::Scalar(2.0));
        assert!(!edited.exposed);

        session.undo();
        let restored = session
            .effect
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .unwrap();
        assert_eq!(restored.name, "Intensity");
        assert_eq!(restored.default, Value::Scalar(1.0));
        assert!(restored.exposed);
    }

    #[test]
    fn public_property_action_is_progressively_disclosed() {
        let mut app = App::new();
        app.add_systems(Update, sync_module_input_public_toggle_visibility);

        let private_row = app
            .world_mut()
            .spawn((ModuleInputPublicRow, RelativeCursorPosition::default()))
            .id();
        let private_toggle = app
            .world_mut()
            .spawn((
                ModuleInputPublicToggle { is_public: false },
                Visibility::Inherited,
            ))
            .id();
        app.world_mut()
            .entity_mut(private_row)
            .add_child(private_toggle);

        let public_row = app
            .world_mut()
            .spawn((ModuleInputPublicRow, RelativeCursorPosition::default()))
            .id();
        let public_toggle = app
            .world_mut()
            .spawn((
                ModuleInputPublicToggle { is_public: true },
                Visibility::Hidden,
            ))
            .id();
        app.world_mut()
            .entity_mut(public_row)
            .add_child(public_toggle);

        app.update();
        assert_eq!(
            app.world().get::<Visibility>(private_toggle),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(public_toggle),
            Some(&Visibility::Inherited)
        );

        *app.world_mut()
            .get_mut::<RelativeCursorPosition>(private_row)
            .unwrap() = RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        };
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(private_toggle),
            Some(&Visibility::Inherited)
        );
    }

    #[test]
    fn public_toggle_preserves_the_default_binding_and_parameter_identity() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        clear_effect_parameters_and_bindings(&mut session);
        let registry = ModuleRegistry::builtin();
        let module = session.selected_layer().modules[0].id;
        let module_type = session.selected_layer().modules[0].module_type.clone();
        let input = registry
            .get(&module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "spawn_rate")
            .unwrap() as u8;

        assert!(toggle_module_input_public(
            &mut session,
            &registry,
            module,
            input,
            &test_localizer(),
        ));
        let parameter_id = session.effect.parameters[0].id;
        let default = session.effect.parameters[0].default.clone();
        assert!(session.effect.parameters[0].exposed);

        assert!(toggle_module_input_public(
            &mut session,
            &registry,
            module,
            input,
            &test_localizer(),
        ));
        assert_eq!(session.effect.parameters[0].id, parameter_id);
        assert_eq!(session.effect.parameters[0].default, default);
        assert!(!session.effect.parameters[0].exposed);
        assert_eq!(
            session.selected_layer().modules[0].bindings["spawn_rate"],
            parameter_id
        );

        session.undo();
        assert!(session.effect.parameters[0].exposed);
        assert_eq!(session.effect.parameters[0].id, parameter_id);
    }

    #[test]
    fn curve_property_source_switches_are_undoable_and_keep_a_valid_value() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "size").is_some())
            .unwrap()
            .id;
        let module_type = session
            .selected_layer()
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .unwrap()
            .module_type
            .clone();
        let input = registry
            .get(&module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "size")
            .unwrap() as u8;
        let Value::Curve(original) = properties_module_parameter(&session, module, "size").unwrap()
        else {
            panic!("size should be curve-authored");
        };
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Constant,
            &test_localizer(),
        ));
        let Value::Curve(constant) = properties_module_parameter(&session, module, "size").unwrap()
        else {
            panic!("constant source remains a typed scalar curve");
        };
        assert_eq!(constant.keys, original.keys);
        assert_ne!(constant.id, original.id);
        assert_eq!(
            session
                .selected_layer()
                .modules
                .iter()
                .find(|candidate| candidate.id == module)
                .unwrap()
                .property_source("size"),
            Some(PropertySourceKind::Constant)
        );

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::ParticleLife),
            &test_localizer(),
        ));
        let Value::Curve(curve) = properties_module_parameter(&session, module, "size").unwrap()
        else {
            panic!("curve source should remain typed");
        };
        assert_eq!(curve, original);
        assert_eq!(
            session
                .selected_layer()
                .modules
                .iter()
                .find(|candidate| candidate.id == module)
                .unwrap()
                .property_source("size"),
            Some(PropertySourceKind::Curve(
                InputEvaluationDomain::ParticleLife
            ))
        );

        session.undo();
        let Value::Curve(restored_constant) =
            properties_module_parameter(&session, module, "size").unwrap()
        else {
            panic!("undo should restore the constant source");
        };
        assert_eq!(restored_constant, constant);
        assert_eq!(
            session
                .selected_layer()
                .modules
                .iter()
                .find(|candidate| candidate.id == module)
                .unwrap()
                .property_source("size"),
            Some(PropertySourceKind::Constant)
        );
    }

    #[test]
    fn constant_curve_source_uses_the_standard_numeric_editor() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "opacity").is_some())
            .unwrap()
            .id;
        let module_type = session
            .selected_layer()
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .unwrap()
            .module_type
            .clone();
        let input = registry
            .get(&module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "opacity")
            .unwrap() as u8;
        let original = properties_module_parameter(&session, module, "opacity").unwrap();
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Constant,
            &test_localizer(),
        ));
        let control = PropertiesNumberControl {
            module,
            parameter: "opacity",
            component: 0,
            kind: PropertiesNumberKind::CurveConstant,
            step: 0.05,
            min: Some(0.0),
            max: Some(1.0),
        };

        assert!(apply_properties_number(
            &mut session,
            control,
            0.65,
            &test_localizer(),
        ));
        assert_eq!(
            properties_number_input_value(&session, control),
            Some(NumberInputValue::F32(0.65))
        );
        let Value::Curve(curve) = properties_module_parameter(&session, module, "opacity").unwrap()
        else {
            panic!("constant source remains curve-typed");
        };
        assert_eq!(curve.keys.len(), 1);
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::ParticleLife),
            &test_localizer(),
        ));
        let Some(Value::Curve(restored)) = properties_module_parameter(&session, module, "opacity")
        else {
            unreachable!()
        };
        let Value::Curve(original) = original else {
            unreachable!()
        };
        assert_eq!(restored.keys, original.keys);
    }

    #[test]
    fn exposing_a_curve_property_copies_it_with_a_distinct_semantic_identity() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        clear_effect_parameters_and_bindings(&mut session);
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.parameter_value("opacity").is_some())
            .unwrap()
            .id;
        let module_type = session
            .selected_layer()
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .unwrap()
            .module_type
            .clone();
        let input = registry
            .get(&module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "opacity")
            .unwrap() as u8;
        let Value::Curve(local) = module_parameter(
            session
                .selected_layer()
                .modules
                .iter()
                .find(|candidate| candidate.id == module)
                .unwrap(),
            "opacity",
        )
        .unwrap() else {
            unreachable!()
        };

        assert!(expose_module_input(
            &mut session,
            &registry,
            module,
            input,
            &test_localizer(),
        ));
        let Value::Curve(public) = &session.effect.parameters[0].default else {
            unreachable!()
        };
        assert_ne!(public.id, local.id);
        assert!(session.effect.validation_report().is_valid());
    }

    #[test]
    fn random_range_source_switches_to_a_constant_and_back() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "lifetime").is_some())
            .unwrap()
            .id;
        let module_type = session
            .selected_layer()
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .unwrap()
            .module_type
            .clone();
        let input = registry
            .get(&module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "lifetime")
            .unwrap() as u8;

        let Value::Range(original) =
            properties_module_parameter(&session, module, "lifetime").unwrap()
        else {
            panic!("lifetime should be range-authored");
        };
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Constant,
            &test_localizer(),
        ));
        let Value::Range(constant) =
            properties_module_parameter(&session, module, "lifetime").unwrap()
        else {
            panic!("lifetime should remain range-typed");
        };
        assert_eq!(constant, original);

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::RandomRange,
            &test_localizer(),
        ));
        let Value::Range(random) =
            properties_module_parameter(&session, module, "lifetime").unwrap()
        else {
            panic!("random lifetime should remain range-typed");
        };
        assert_eq!(random, original);
        assert!(session.can_undo());
    }

    #[test]
    fn spawn_rate_sources_preserve_constant_random_and_emitter_curve_values() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        clear_effect_parameters_and_bindings(&mut session);
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.parameter_value("spawn_rate").is_some())
            .unwrap()
            .id;
        let module_type = session
            .selected_layer()
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .unwrap()
            .module_type
            .clone();
        let input = registry
            .get(&module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "spawn_rate")
            .unwrap() as u8;
        let original = properties_module_parameter(&session, module, "spawn_rate").unwrap();

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::RandomRange,
            &test_localizer(),
        ));
        let random = properties_module_parameter(&session, module, "spawn_rate").unwrap();
        assert!(matches!(random, Value::Range(_)));

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::EmitterTime),
            &test_localizer(),
        ));
        let curve = properties_module_parameter(&session, module, "spawn_rate").unwrap();
        assert!(matches!(curve, Value::Curve(_)));

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Constant,
            &test_localizer(),
        ));
        assert_eq!(
            properties_module_parameter(&session, module, "spawn_rate"),
            Some(original)
        );

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::RandomRange,
            &test_localizer(),
        ));
        assert_eq!(
            properties_module_parameter(&session, module, "spawn_rate"),
            Some(random)
        );
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::EmitterTime),
            &test_localizer(),
        ));
        assert_eq!(
            properties_module_parameter(&session, module, "spawn_rate"),
            Some(curve)
        );
        assert!(
            session
                .effect
                .to_pretty_ron()
                .unwrap()
                .contains("property_source_values")
        );
    }

    #[test]
    fn public_spawn_rate_tracks_the_active_source_without_losing_alternates() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        clear_effect_parameters_and_bindings(&mut session);
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.parameter_value("spawn_rate").is_some())
            .unwrap()
            .id;
        let module_type = session
            .selected_layer()
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .unwrap()
            .module_type
            .clone();
        let input = registry
            .get(&module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "spawn_rate")
            .unwrap() as u8;
        assert!(expose_module_input(
            &mut session,
            &registry,
            module,
            input,
            &test_localizer(),
        ));
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::RandomRange,
            &test_localizer(),
        ));
        let random = session.effect.parameters[0].default.clone();
        assert!(matches!(random, Value::Range(_)));
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::EmitterTime),
            &test_localizer(),
        ));
        assert!(matches!(
            session.effect.parameters[0].default,
            Value::Curve(_)
        ));
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::RandomRange,
            &test_localizer(),
        ));
        assert_eq!(session.effect.parameters[0].default, random);
        assert!(session.effect.validation_report().is_valid());
    }

    #[test]
    fn editing_an_exposed_property_updates_its_source_default_in_place() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        clear_effect_parameters_and_bindings(&mut session);
        let registry = ModuleRegistry::builtin();
        let module = session.selected_layer().modules[0].id;
        let module_type = session.selected_layer().modules[0].module_type.clone();
        let input = registry
            .get(&module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "spawn_rate")
            .unwrap() as u8;
        let local = module_parameter(&session.selected_layer().modules[0], "spawn_rate").unwrap();
        assert!(expose_module_input(
            &mut session,
            &registry,
            module,
            input,
            &test_localizer(),
        ));
        let control = PropertiesNumberControl {
            module,
            parameter: "spawn_rate",
            component: 0,
            kind: PropertiesNumberKind::Scalar,
            step: 1.0,
            min: Some(0.0),
            max: None,
        };

        assert!(apply_properties_number(
            &mut session,
            control,
            42.0,
            &test_localizer(),
        ));
        assert_eq!(session.effect.parameters[0].default, Value::Scalar(42.0));
        assert_eq!(
            properties_module_parameter(&session, module, "spawn_rate"),
            Some(Value::Scalar(42.0))
        );
        assert_eq!(
            module_parameter(&session.selected_layer().modules[0], "spawn_rate"),
            Some(local.clone()),
            "the hidden local value is not a second source of truth once bound"
        );
        session.undo();
        assert_eq!(session.effect.parameters[0].default, local);
    }

    #[test]
    fn exposing_module_input_creates_public_parameter_and_binding_atomically() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        clear_effect_parameters_and_bindings(&mut session);
        let registry = ModuleRegistry::builtin();
        let module = session.selected_layer().modules[0].id;
        let module_type = session.selected_layer().modules[0].module_type.clone();
        let metadata = registry.get(&module_type).unwrap();
        let input = metadata
            .inputs
            .iter()
            .position(|input| input.name == "spawn_rate")
            .unwrap() as u8;
        let local_value = module_parameter(&session.selected_layer().modules[0], "spawn_rate");

        assert!(expose_module_input(
            &mut session,
            &registry,
            module,
            input,
            &test_localizer()
        ));

        assert_eq!(session.effect.parameters.len(), 1);
        let parameter = &session.effect.parameters[0];
        assert!(parameter.exposed);
        assert_eq!(Some(parameter.default.clone()), local_value);
        assert_eq!(
            session.selected_layer().modules[0].bindings["spawn_rate"],
            parameter.id
        );
        session.undo();
        assert!(session.effect.parameters.is_empty());
        assert!(session.selected_layer().modules[0].bindings.is_empty());
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
        let effect_name = app.world_mut().spawn(DocumentTextControl::Effect).id();
        let emitter_name = app.world_mut().spawn(DocumentTextControl::Emitter).id();

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
    fn properties_action_activation_uses_the_feathers_contract() {
        let mut app = App::new();
        app.add_observer(queue_properties_action_activation);
        let action = app
            .world_mut()
            .spawn((
                PropertiesAction::CloseModulePalette,
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
    fn properties_actions_are_executed_by_the_properties_plugin_path() {
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
        .add_systems(Update, handle_properties_actions);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            PropertiesAction::OpenModulePalette(StackStage::ParticleSpawn),
            BackgroundColor(theme::BUTTON),
        ));

        app.update();

        let palette = app.world().resource::<ModulePaletteState>();
        assert!(palette.open);
        assert_eq!(palette.stage, StackStage::ParticleSpawn);
    }

    #[test]
    fn properties_disclosure_persists_without_requesting_a_ui_rebuild() {
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
            .add_systems(Update, handle_properties_actions);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            PropertiesAction::ToggleSection(PropertiesSection::Module(module)),
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
    fn properties_event_action_creates_an_undoable_semantic_link() {
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
            .add_systems(Update, handle_properties_actions);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            PropertiesAction::AddEventLink {
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
    fn properties_outcomes_are_localized_and_preserve_semantic_details() {
        let english = test_localizer();
        assert_eq!(
            localize_properties_status(PropertiesStatus::TargetUnavailable, &english),
            "Properties target is no longer available"
        );
        let finite = localize_properties_status(
            PropertiesStatus::FiniteNumberRequired("spawn_rate".into()),
            &english,
        );
        assert!(finite.contains("spawn_rate"));
        assert!(finite.ends_with(" requires a finite number"));

        let french = Localizer::new("fr-FR").unwrap();
        assert_eq!(
            localize_properties_status(PropertiesStatus::ChoiceUnavailable, &french),
            "Le choix n’est plus disponible"
        );
        let selected =
            localize_properties_status(PropertiesStatus::Selected("module/shape".into()), &french);
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

    // Properties domain tests.
    #[test]
    fn compiled_navigation_focuses_the_exact_properties_target() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let target = SemanticTarget::Module(session.effect.emitters[3].modules[2].id);
        let mut focus = PropertiesFocus::default();

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
        assert_eq!(focus.highlight_remaining, PROPERTIES_HIGHLIGHT_DURATION);
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
    fn properties_input_localization_uses_fluent_and_preserves_custom_metadata() {
        let localizer = Localizer::new("fr-FR").unwrap();
        assert_eq!(
            localized_properties_input(&localizer, "spawn_rate", "Spawn Rate", false),
            "Taux d’émission"
        );
        assert_eq!(
            localized_properties_input(&localizer, "custom_gain", "Custom Gain", false),
            "Custom Gain"
        );
    }

    #[test]
    fn property_units_are_presented_in_the_tooltip_footer() {
        let localizer = test_localizer();
        let tooltip = property_tooltip(
            "Rotation applied to each particle.",
            Some("rad/s"),
            &localizer,
        );
        let accessible = tooltip.accessible_label();
        assert!(accessible.starts_with("Rotation applied to each particle."));
        assert!(accessible.contains("Unit:"), "{accessible:?}");
        assert!(accessible.contains("rad/s"), "{accessible:?}");

        let unitless = property_tooltip("Normalized opacity.", None, &localizer);
        assert_eq!(unitless.accessible_label(), "Normalized opacity.");
    }

    #[test]
    fn properties_number_rejects_non_finite_values() {
        assert_eq!(clamp_properties_number(f32::INFINITY, None, None), None);
        assert_eq!(
            clamp_properties_number(-5.0, Some(0.0), Some(10.0)),
            Some(0.0)
        );
    }

    #[test]
    fn properties_typing_does_not_rebuild_or_commit_until_final() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let original = properties_module_parameter(&session, module, "spawn_rate").unwrap();
        let revision = session.ui_revision;
        let mut app = App::new();
        app.insert_resource(session);
        app.insert_resource(test_localizer());
        app.add_observer(handle_properties_scalar_change);
        let control = app
            .world_mut()
            .spawn(PropertiesNumberControl {
                module,
                parameter: "spawn_rate",
                component: 0,
                kind: PropertiesNumberKind::Scalar,
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
            properties_module_parameter(session, module, "spawn_rate"),
            Some(original)
        );
        assert_eq!(session.ui_revision, revision);
    }

    #[test]
    fn properties_range_edit_preserves_ordering() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "lifetime").is_some())
            .unwrap()
            .id;
        let control = PropertiesNumberControl {
            module,
            parameter: "lifetime",
            component: 0,
            kind: PropertiesNumberKind::Range,
            step: 0.1,
            min: Some(0.05),
            max: None,
        };

        assert!(apply_properties_number(
            &mut session,
            control,
            99.0,
            &test_localizer(),
        ));
        let Value::Range(range) =
            properties_module_parameter(&session, module, "lifetime").unwrap()
        else {
            panic!("lifetime should remain a range");
        };
        assert_eq!(range.min, range.max);
    }

    #[test]
    fn properties_scrub_previews_live_and_commits_one_undoable_edit() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let original = properties_module_parameter(&session, module, "spawn_rate").unwrap();
        let target = NumericScrubTarget::Properties(PropertiesNumberControl {
            module,
            parameter: "spawn_rate",
            component: 0,
            kind: PropertiesNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: None,
        });

        assert!(preview_numeric_scrub(&mut session, target, 29.0));
        assert_eq!(
            properties_module_parameter(&session, module, "spawn_rate"),
            Some(original.clone()),
            "drag preview must not mutate the document"
        );
        commit_numeric_scrub(&mut session, target, 29.0, &test_localizer());
        assert_eq!(
            properties_module_parameter(&session, module, "spawn_rate"),
            Some(Value::Scalar(29.0))
        );
        session.undo();
        assert_eq!(
            properties_module_parameter(&session, module, "spawn_rate"),
            Some(original)
        );
    }

    #[test]
    fn marker_offset_scrub_preserves_binding_and_commits_one_undoable_edit() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let emitter = session.selected_layer().id;
        assert!(session.execute(
            "Shorten emitter",
            EffectCommand::SetEmitterTiming {
                id: emitter,
                start_time: 0.0,
                duration: 1.0,
            },
            true,
        ));
        let marker = EffectMarker::new("Impact", 0.5);
        let marker_id = marker.id;
        assert!(session.execute(
            "Add marker",
            EffectCommand::AddMarker { marker, index: 0 },
            true,
        ));
        set_start_reference(
            &mut session,
            StartReferenceTarget::Emitter(emitter),
            Some(marker_id),
            &test_localizer(),
        );
        let target = NumericScrubTarget::StartReferenceOffset(StartReferenceOffsetControl {
            target: StartReferenceTarget::Emitter(emitter),
        });
        let original = session.selected_layer().start_reference.unwrap();

        assert_eq!(numeric_scrub_step(target), 0.05);
        assert!(preview_numeric_scrub(&mut session, target, 0.25));
        assert_eq!(session.selected_layer().start_reference, Some(original));
        commit_numeric_scrub(&mut session, target, 0.25, &test_localizer());
        assert_eq!(
            session.selected_layer().start_reference.unwrap().offset,
            0.25
        );
        assert_eq!(session.selected_layer().start_time, 0.75);

        session.undo();
        assert_eq!(session.selected_layer().start_reference, Some(original));
        assert_eq!(session.selected_layer().start_time, 0.0);
    }

    #[test]
    fn bounded_slider_commit_preserves_the_properties_tree_and_is_undoable() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spread_degrees").is_some())
            .unwrap()
            .id;
        let original = properties_module_parameter(&session, module, "spread_degrees").unwrap();
        let target = NumericScrubTarget::Properties(PropertiesNumberControl {
            module,
            parameter: "spread_degrees",
            component: 0,
            kind: PropertiesNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: Some(360.0),
        });
        let ui_revision = session.ui_revision;

        assert!(preview_numeric_scrub(&mut session, target, 75.0));
        assert!(commit_bounded_slider(&mut session, target, 75.0));
        assert_eq!(session.ui_revision, ui_revision);
        assert_eq!(
            properties_module_parameter(&session, module, "spread_degrees"),
            Some(Value::Scalar(75.0))
        );

        session.undo();
        assert_eq!(
            properties_module_parameter(&session, module, "spread_degrees"),
            Some(original)
        );
    }

    #[test]
    fn properties_scrub_uses_metadata_steps_and_modifier_precision() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let control = PropertiesNumberControl {
            module,
            parameter: "spawn_rate",
            component: 0,
            kind: PropertiesNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: None,
        };
        let target = NumericScrubTarget::Properties(control);
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
    fn properties_sections_use_compact_defaults_and_persist_type_preferences() {
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

        assert!(!properties_module_collapsed(&settings, emission));
        assert!(properties_module_collapsed(&settings, motion));
        assert!(properties_renderer_collapsed(&settings, renderer));

        assert!(toggle_persisted_properties_section(
            &session,
            &mut settings,
            PropertiesSection::Module(motion.id),
        ));
        assert!(!properties_module_collapsed(&settings, motion));
        assert_eq!(
            settings
                .properties
                .section_expansion
                .get(&properties_module_key(motion)),
            Some(&true)
        );

        assert!(toggle_persisted_properties_section(
            &session,
            &mut settings,
            PropertiesSection::Renderer(renderer.id),
        ));
        assert!(!properties_renderer_collapsed(&settings, renderer));
        assert_eq!(
            settings
                .properties
                .section_expansion
                .get(&properties_renderer_key(renderer)),
            Some(&true)
        );
    }

    #[test]
    fn properties_number_edit_is_clamped_semantic_and_undoable() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let original = properties_module_parameter(&session, module, "spawn_rate").unwrap();
        let control = PropertiesNumberControl {
            module,
            parameter: "spawn_rate",
            component: 0,
            kind: PropertiesNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: Some(30.0),
        };

        assert!(apply_properties_number(
            &mut session,
            control,
            300.0,
            &test_localizer(),
        ));
        assert_eq!(
            properties_module_parameter(&session, module, "spawn_rate"),
            Some(Value::Scalar(30.0))
        );
        assert!(session.can_undo());

        session.undo();
        assert_eq!(
            properties_module_parameter(&session, module, "spawn_rate"),
            Some(original)
        );
    }

    #[test]
    fn properties_edits_volumetric_shape_dimensions_semantically() {
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

        let control = PropertiesNumberControl {
            module,
            parameter: "shape",
            component: 2,
            kind: PropertiesNumberKind::Shape,
            step: 0.1,
            min: Some(0.1),
            max: None,
        };
        assert!(apply_properties_number(
            &mut session,
            control,
            18.0,
            &test_localizer(),
        ));
        assert_eq!(
            properties_module_parameter(&session, module, "shape"),
            Some(Value::Shape(EmitterShape::Box {
                half_extents: [12.0, 12.0, 18.0],
            }))
        );
    }

    #[test]
    fn properties_choice_selects_the_requested_shape_directly() {
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
            properties_module_parameter(&session, module, "shape"),
            Some(Value::Shape(EmitterShape::Cone {
                radius: 12.0,
                depth: 24.0,
            }))
        );
    }

    #[test]
    fn properties_emitter_transform_components_are_semantic_and_undoable() {
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
    fn properties_effect_clip_transform_is_semantic_and_undoable() {
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

    #[test]
    fn nested_reference_breadcrumbs_include_every_effect_level() {
        let temporary = tempfile::tempdir().unwrap();
        let mut leaf = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        leaf.id = aestra_bevy::EffectId::from_u128(0x1EAF);
        leaf.name = "Leaf".into();
        leaf.effect_clips.clear();
        leaf.save_ron(temporary.path().join("leaf.aestra.ron"))
            .unwrap();

        let mut child = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        child.id = aestra_bevy::EffectId::from_u128(0xC111D);
        child.name = "Child".into();
        child.effect_clips.clear();
        let nested = EffectClip::new(EffectAssetRef::new(leaf.id), 0.0, 1.0);
        let nested_id = nested.id;
        child.effect_clips.push(nested);
        child
            .save_ron(temporary.path().join("child.aestra.ron"))
            .unwrap();

        let mut root = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        root.id = aestra_bevy::EffectId::from_u128(0xA007);
        root.name = "Root".into();
        root.effect_clips.clear();
        let parent = EffectClip::new(EffectAssetRef::new(child.id), 0.0, 1.0);
        let path = EffectClipPath::root_path(parent.id).child(nested_id);
        root.effect_clips.push(parent);
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.effect = root;
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let breadcrumbs = effect_clip_breadcrumbs(&session, &catalog, &path);

        assert_eq!(
            breadcrumbs
                .iter()
                .map(|(label, _)| label.clone())
                .collect::<Vec<_>>(),
            ["Root", "Child", "Leaf"]
        );
        assert_eq!(breadcrumbs[0].1, None);
        assert_eq!(
            breadcrumbs[1].1,
            Some(DocumentAction::OpenSource(EffectAssetRef::new(child.id)))
        );
        assert_eq!(
            breadcrumbs[2].1,
            Some(DocumentAction::OpenSource(EffectAssetRef::new(leaf.id)))
        );
    }
}
fn scroll_properties_to_focus(
    mut commands: Commands,
    mut focus: ResMut<PropertiesFocus>,
    targets: Query<(Entity, &PropertiesSemanticTarget)>,
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

fn update_properties_highlight(
    time: Res<Time>,
    mut focus: ResMut<PropertiesFocus>,
    mut targets: Query<(&PropertiesSemanticTarget, &mut BorderColor)>,
) {
    let Some(highlight) = focus.highlight else {
        return;
    };
    focus.highlight_remaining = (focus.highlight_remaining - time.delta_secs()).max(0.0);
    let strength = (focus.highlight_remaining / PROPERTIES_HIGHLIGHT_DURATION)
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
        set_properties_status(session, localizer, PropertiesStatus::ModuleMissing);
        return;
    };
    let Some(input) = registry
        .get(&module.module_type)
        .and_then(|metadata| metadata.inputs.get(input_index as usize))
    else {
        set_properties_status(
            session,
            localizer,
            PropertiesStatus::InputMetadataUnavailable,
        );
        return;
    };
    if !matches!(input.control, InputControl::Choice) {
        set_properties_status(
            session,
            localizer,
            PropertiesStatus::NotChoice(input.display_name.into()),
        );
        return;
    }
    let current = properties_module_parameter(session, module_id, input.name);
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
            set_properties_status(session, localizer, PropertiesStatus::ChoiceUnavailable);
            return;
        }
    };
    if let Some(command) =
        properties_module_parameter_command(session, module_id, input.name, Value::Shape(shape))
    {
        session.execute(
            localizer.text("properties-edit-module-input-command"),
            command,
            true,
        );
    }
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

fn sync_marker_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &MarkerNumberControl), Added<MarkerNumberControl>>,
) {
    for (entity, control) in &controls {
        if let Some(marker) = session
            .effect
            .markers
            .iter()
            .find(|marker| marker.id == control.0)
        {
            commands.trigger(UpdateNumberInput {
                entity,
                value: NumberInputValue::F32(marker.time),
            });
        }
    }
}

fn sync_choreography_event_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<
        (Entity, &ChoreographyEventNumberControl),
        Added<ChoreographyEventNumberControl>,
    >,
) {
    for (entity, control) in &controls {
        let (id, intensity) = match *control {
            ChoreographyEventNumberControl::Time(id) => (id, false),
            ChoreographyEventNumberControl::Intensity(id) => (id, true),
        };
        let Some(event) = session
            .effect
            .choreography_events
            .iter()
            .find(|event| event.id == id)
        else {
            continue;
        };
        let value = if intensity {
            match event.payload {
                ChoreographyEventPayload::CameraShake { intensity } => intensity,
                _ => continue,
            }
        } else {
            event.time
        };
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::F32(value),
        });
    }
}

fn sync_start_reference_offset_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &StartReferenceOffsetControl), Added<StartReferenceOffsetControl>>,
) {
    for (entity, control) in &controls {
        let reference = start_reference(&session, control.target);
        if let Some(reference) = reference {
            commands.trigger(UpdateNumberInput {
                entity,
                value: NumberInputValue::F32(reference.offset),
            });
        }
    }
}

fn sync_effect_clip_parameter_number_inputs(
    mut commands: Commands,
    controls: Query<
        (Entity, &EffectClipParameterNumberControl),
        Added<EffectClipParameterNumberControl>,
    >,
) {
    for (entity, control) in &controls {
        let value = match &control.value {
            Value::U32(value) => NumberInputValue::I32((*value).min(i32::MAX as u32) as i32),
            Value::Scalar(value) => NumberInputValue::F32(*value),
            Value::Vec2(value) => NumberInputValue::F32(value[control.component as usize]),
            Value::Vec3(value) => NumberInputValue::F32(value[control.component as usize]),
            Value::Vec4(value) => NumberInputValue::F32(value[control.component as usize]),
            Value::Range(value) => NumberInputValue::F32(if control.component == 0 {
                value.min
            } else {
                value.max
            }),
            _ => continue,
        };
        commands.trigger(UpdateNumberInput { entity, value });
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

fn sync_properties_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &PropertiesNumberControl), Added<PropertiesNumberControl>>,
) {
    for (entity, control) in &controls {
        let Some(value) = properties_number_input_value(&session, *control) else {
            continue;
        };
        commands.trigger(UpdateNumberInput { entity, value });
    }
}

fn sync_properties_slider_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &PropertiesSliderControl), Added<PropertiesSliderControl>>,
) {
    for (entity, control) in &controls {
        let Some(NumberInputValue::F32(value)) = properties_number_input_value(&session, control.0)
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

fn properties_number_input_value(
    session: &EditorSession,
    control: PropertiesNumberControl,
) -> Option<NumberInputValue> {
    let value = properties_module_parameter(session, control.module, control.parameter)?;
    match (control.kind, value) {
        (PropertiesNumberKind::U32, Value::U32(value)) => {
            Some(NumberInputValue::I32(value.min(i32::MAX as u32) as i32))
        }
        (PropertiesNumberKind::Scalar, Value::Scalar(value)) => Some(NumberInputValue::F32(value)),
        (PropertiesNumberKind::CurveConstant, Value::Curve(curve)) => curve
            .keys
            .first()
            .map(|key| NumberInputValue::F32(key.value)),
        (PropertiesNumberKind::Vector, Value::Vec2(value)) => value
            .get(control.component as usize)
            .copied()
            .map(NumberInputValue::F32),
        (PropertiesNumberKind::Vector, Value::Vec3(value)) => value
            .get(control.component as usize)
            .copied()
            .map(NumberInputValue::F32),
        (PropertiesNumberKind::Vector, Value::Vec4(value)) => value
            .get(control.component as usize)
            .copied()
            .map(NumberInputValue::F32),
        (PropertiesNumberKind::Range, Value::Range(value)) => {
            Some(NumberInputValue::F32(if control.component == 0 {
                value.min
            } else {
                value.max
            }))
        }
        (PropertiesNumberKind::RangeConstant, Value::Range(value)) => {
            Some(NumberInputValue::F32((value.min + value.max) * 0.5))
        }
        (PropertiesNumberKind::Shape, Value::Shape(shape)) => {
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
            DocumentTextControl::Effect => localizer.text("properties-effect"),
            DocumentTextControl::Emitter => localizer.text("properties-emitter"),
            DocumentTextControl::Marker(_) => localizer.text("properties-marker"),
            DocumentTextControl::ChoreographyEvent(_) => {
                localizer.text("properties-choreography-event")
            }
        };
        set_properties_status(
            &mut session,
            &localizer,
            PropertiesStatus::NameRequired(target),
        );
        session.ui_revision += 1;
        return;
    }
    let changed = match control {
        DocumentTextControl::Effect => session.set_effect_name(value),
        DocumentTextControl::Emitter => session.set_selected_emitter_name(value),
        DocumentTextControl::Marker(id) => session.execute(
            "Renamed timeline marker",
            EffectCommand::SetMarkerName {
                id: *id,
                name: value.to_owned(),
            },
            true,
        ),
        DocumentTextControl::ChoreographyEvent(id) => session.execute(
            "Renamed choreography event",
            EffectCommand::SetChoreographyEventName {
                id: *id,
                name: value.to_owned(),
            },
            true,
        ),
    };
    if changed {
        let target = match control {
            DocumentTextControl::Effect => localizer.text("properties-effect-name-status-target"),
            DocumentTextControl::Emitter => localizer.text("properties-emitter-name-status-target"),
            DocumentTextControl::Marker(_) => localizer.text("properties-marker"),
            DocumentTextControl::ChoreographyEvent(_) => {
                localizer.text("properties-choreography-event")
            }
        };
        set_properties_status(&mut session, &localizer, PropertiesStatus::Updated(target));
    }
}

fn handle_choreography_event_payload_text_change(
    change: On<ValueChange<String>>,
    controls: Query<&ChoreographyEventPayloadTextControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let Some(event) = session
        .effect
        .choreography_events
        .iter()
        .find(|event| event.id == control.0)
    else {
        return;
    };
    let value = change.value.trim().to_owned();
    let payload = match event.payload {
        ChoreographyEventPayload::GameplayNotify { .. } => {
            ChoreographyEventPayload::GameplayNotify { topic: value }
        }
        ChoreographyEventPayload::PlaySound { .. } => {
            ChoreographyEventPayload::PlaySound { cue: value }
        }
        ChoreographyEventPayload::SpawnChildEffect { .. } => {
            ChoreographyEventPayload::SpawnChildEffect { effect: value }
        }
        ChoreographyEventPayload::CameraShake { .. } => return,
    };
    session.execute(
        "Changed choreography event payload",
        EffectCommand::SetChoreographyEventPayload {
            id: control.0,
            payload,
        },
        true,
    );
}

fn handle_choreography_event_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&ChoreographyEventNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    match *control {
        ChoreographyEventNumberControl::Time(id) => {
            let time = change.value.clamp(0.0, session.playback_duration());
            session.execute(
                "Moved choreography event",
                EffectCommand::SetChoreographyEventTime { id, time },
                true,
            );
        }
        ChoreographyEventNumberControl::Intensity(id) => {
            session.execute(
                "Changed camera shake intensity",
                EffectCommand::SetChoreographyEventPayload {
                    id,
                    payload: ChoreographyEventPayload::CameraShake {
                        intensity: change.value.max(0.0),
                    },
                },
                true,
            );
        }
    }
}

fn handle_marker_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&MarkerNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let time = change.value.clamp(0.0, session.playback_duration());
    let Some(marker) = session
        .effect
        .markers
        .iter()
        .find(|marker| marker.id == control.0)
    else {
        return;
    };
    if (marker.time - time).abs() <= f32::EPSILON {
        return;
    }
    session.execute(
        "Moved timeline marker",
        EffectCommand::SetMarkerTime {
            id: control.0,
            time,
        },
        true,
    );
}

fn handle_start_reference_offset_change(
    change: On<ValueChange<f32>>,
    controls: Query<&StartReferenceOffsetControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let Some(mut reference) = start_reference(&session, control.target) else {
        return;
    };
    let offset = normalize_start_reference_offset(&session, control.target, change.value);
    if (reference.offset - offset).abs() <= f32::EPSILON {
        return;
    }
    reference.offset = offset;
    execute_start_reference(&mut session, control.target, Some(reference), &localizer);
}

fn start_reference(
    session: &EditorSession,
    target: StartReferenceTarget,
) -> Option<MarkerTimeReference> {
    match target {
        StartReferenceTarget::Emitter(id) => session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == id)
            .and_then(|emitter| emitter.start_reference),
        StartReferenceTarget::EffectClip(id) => session
            .effect
            .effect_clips
            .iter()
            .find(|clip| clip.id == id)
            .and_then(|clip| clip.start_reference),
        StartReferenceTarget::ChoreographyEvent(id) => session
            .effect
            .choreography_events
            .iter()
            .find(|event| event.id == id)
            .and_then(|event| event.time_reference),
    }
}

fn target_start_time(session: &EditorSession, target: StartReferenceTarget) -> Option<f32> {
    match target {
        StartReferenceTarget::Emitter(id) => session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == id)
            .map(|emitter| emitter.start_time),
        StartReferenceTarget::EffectClip(id) => session
            .effect
            .effect_clips
            .iter()
            .find(|clip| clip.id == id)
            .map(|clip| clip.start_time),
        StartReferenceTarget::ChoreographyEvent(id) => session
            .effect
            .choreography_events
            .iter()
            .find(|event| event.id == id)
            .map(|event| event.time),
    }
}

fn target_duration(session: &EditorSession, target: StartReferenceTarget) -> Option<f32> {
    match target {
        StartReferenceTarget::Emitter(id) => session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == id)
            .map(|emitter| emitter.duration),
        StartReferenceTarget::EffectClip(id) => session
            .effect
            .effect_clips
            .iter()
            .find(|clip| clip.id == id)
            .map(|clip| clip.duration),
        StartReferenceTarget::ChoreographyEvent(_) => Some(0.0),
    }
}

fn normalize_start_reference_offset(
    session: &EditorSession,
    target: StartReferenceTarget,
    offset: f32,
) -> f32 {
    let Some(reference) = start_reference(session, target) else {
        return offset;
    };
    let Some(marker_time) = session
        .effect
        .markers
        .iter()
        .find(|marker| marker.id == reference.marker)
        .map(|marker| marker.time)
    else {
        return offset;
    };
    let duration = target_duration(session, target).unwrap_or_default();
    let min_offset = -marker_time;
    let max_offset = session.effect.duration - duration - marker_time;
    offset.clamp(min_offset, max_offset.max(min_offset))
}

fn set_start_reference(
    session: &mut EditorSession,
    target: StartReferenceTarget,
    marker: Option<MarkerId>,
    localizer: &Localizer,
) {
    let reference = marker.and_then(|marker| {
        let start = target_start_time(session, target)?;
        let marker_time = session
            .effect
            .markers
            .iter()
            .find(|candidate| candidate.id == marker)?
            .time;
        Some(MarkerTimeReference::new(marker, start - marker_time))
    });
    execute_start_reference(session, target, reference, localizer);
}

fn execute_start_reference(
    session: &mut EditorSession,
    target: StartReferenceTarget,
    reference: Option<MarkerTimeReference>,
    localizer: &Localizer,
) {
    let command = match target {
        StartReferenceTarget::Emitter(id) => {
            EffectCommand::SetEmitterStartReference { id, reference }
        }
        StartReferenceTarget::EffectClip(id) => {
            EffectCommand::SetEffectClipStartReference { id, reference }
        }
        StartReferenceTarget::ChoreographyEvent(id) => {
            EffectCommand::SetChoreographyEventTimeReference { id, reference }
        }
    };
    session.execute(
        localizer.text("properties-start-reference-command"),
        command,
        true,
    );
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
            localizer.text("properties-emitter-enabled"),
        ),
    };
    if changed {
        set_properties_status(&mut session, &localizer, PropertiesStatus::Updated(target));
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
        set_properties_status(
            &mut session,
            &localizer,
            PropertiesStatus::Updated(localizer.text("properties-emitter-capacity")),
        );
    }
}

fn handle_properties_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&PropertiesToggleControl>,
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
    let Some(current) = properties_module_parameter(&session, control.module, control.parameter)
    else {
        set_properties_status(
            &mut session,
            &localizer,
            PropertiesStatus::TargetUnavailable,
        );
        return;
    };
    let value = Value::Bool(change.value);
    if current != value
        && let Some(command) =
            properties_module_parameter_command(&session, control.module, control.parameter, value)
    {
        session.execute(
            localizer.text("properties-edit-module-input-command"),
            command,
            true,
        );
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
    properties_controls: Query<&PropertiesSliderControl>,
    renderer_controls: Query<&RendererSliderControl>,
    pairs: Query<&SliderNumberInputPair>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<BoundedSliderState>,
) {
    if !change.value.is_finite() {
        return;
    }
    let target = if let Ok(control) = properties_controls.get(change.source) {
        NumericScrubTarget::Properties(control.0)
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

fn handle_effect_clip_parameter_integer_change(
    change: On<ValueChange<i32>>,
    controls: Query<&EffectClipParameterNumberControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let Value::U32(current) = control.value else {
        return;
    };
    let value = change.value.max(0) as u32;
    if value != current {
        set_effect_clip_parameter_override(
            &mut session,
            &localizer,
            control.clip,
            control.parameter,
            Value::U32(value),
        );
    }
}

fn handle_effect_clip_parameter_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&EffectClipParameterNumberControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let Some(value) = effect_clip_parameter_value_with_component(
        control.value.clone(),
        control.component,
        change.value,
    ) else {
        return;
    };
    if value != control.value {
        set_effect_clip_parameter_override(
            &mut session,
            &localizer,
            control.clip,
            control.parameter,
            value,
        );
    }
}

fn handle_effect_clip_parameter_text_change(
    change: On<ValueChange<String>>,
    controls: Query<&EffectClipParameterTextControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value != control.value {
        set_effect_clip_parameter_override(
            &mut session,
            &localizer,
            control.clip,
            control.parameter,
            Value::Text(change.value.clone()),
        );
    }
}

fn handle_effect_clip_parameter_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&EffectClipParameterToggleControl>,
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
    if change.value == control.value {
        return;
    }
    set_effect_clip_parameter_override(
        &mut session,
        &localizer,
        control.clip,
        control.parameter,
        Value::Bool(change.value),
    );
}

fn update_effect_parameter(
    session: &mut EditorSession,
    localizer: &Localizer,
    id: ParameterId,
    update: impl FnOnce(&mut EffectParameter),
) -> bool {
    let Some(mut parameter) = session
        .effect
        .parameters
        .iter()
        .find(|parameter| parameter.id == id)
        .cloned()
    else {
        return false;
    };
    update(&mut parameter);
    session.execute(
        localizer.text("properties-edit-effect-parameter-command"),
        EffectCommand::SetParameter { id, parameter },
        true,
    )
}

fn set_effect_clip_parameter_override(
    session: &mut EditorSession,
    localizer: &Localizer,
    clip: EffectClipId,
    parameter: ParameterId,
    value: Value,
) {
    session.execute(
        localizer.text("properties-set-effect-clip-parameter-command"),
        EffectCommand::SetEffectClipParameterOverride {
            id: clip,
            parameter,
            value,
        },
        true,
    );
}

fn effect_clip_parameter_value_with_component(
    mut value: Value,
    component: u8,
    replacement: f32,
) -> Option<Value> {
    match &mut value {
        Value::Scalar(current) if component == 0 => *current = replacement,
        Value::Vec2(current) if (component as usize) < current.len() => {
            current[component as usize] = replacement;
        }
        Value::Vec3(current) if (component as usize) < current.len() => {
            current[component as usize] = replacement;
        }
        Value::Vec4(current) if (component as usize) < current.len() => {
            current[component as usize] = replacement;
        }
        Value::Range(current) if component == 0 => current.min = replacement,
        Value::Range(current) if component == 1 => current.max = replacement,
        _ => return None,
    }
    Some(value)
}

fn effect_clip_parameter_scrub_control(
    control: &EffectClipParameterNumberControl,
) -> Option<EffectClipParameterScrubControl> {
    let (values, kind) = match &control.value {
        Value::U32(value) => (
            [*value as f32, 0.0, 0.0, 0.0],
            EffectClipParameterScrubKind::U32,
        ),
        Value::Scalar(value) => (
            [*value, 0.0, 0.0, 0.0],
            EffectClipParameterScrubKind::Scalar,
        ),
        Value::Vec2(value) => (
            [value[0], value[1], 0.0, 0.0],
            EffectClipParameterScrubKind::Vec2,
        ),
        Value::Vec3(value) => (
            [value[0], value[1], value[2], 0.0],
            EffectClipParameterScrubKind::Vec3,
        ),
        Value::Vec4(value) => (*value, EffectClipParameterScrubKind::Vec4),
        Value::Range(value) => (
            [value.min, value.max, 0.0, 0.0],
            EffectClipParameterScrubKind::Range,
        ),
        _ => return None,
    };
    if control.component as usize >= effect_clip_parameter_scrub_component_count(kind) {
        return None;
    }
    Some(EffectClipParameterScrubControl {
        clip: control.clip,
        parameter: control.parameter,
        component: control.component,
        values,
        kind,
    })
}

fn effect_clip_parameter_scrub_component_count(kind: EffectClipParameterScrubKind) -> usize {
    match kind {
        EffectClipParameterScrubKind::U32 | EffectClipParameterScrubKind::Scalar => 1,
        EffectClipParameterScrubKind::Vec2 | EffectClipParameterScrubKind::Range => 2,
        EffectClipParameterScrubKind::Vec3 => 3,
        EffectClipParameterScrubKind::Vec4 => 4,
    }
}

fn effect_clip_parameter_scrub_value(
    control: EffectClipParameterScrubControl,
    replacement: f32,
) -> Value {
    let mut values = control.values;
    values[control.component as usize] = replacement;
    match control.kind {
        EffectClipParameterScrubKind::U32 => {
            Value::U32(replacement.max(0.0).round().min(u32::MAX as f32) as u32)
        }
        EffectClipParameterScrubKind::Scalar => Value::Scalar(replacement),
        EffectClipParameterScrubKind::Vec2 => Value::Vec2([values[0], values[1]]),
        EffectClipParameterScrubKind::Vec3 => Value::Vec3([values[0], values[1], values[2]]),
        EffectClipParameterScrubKind::Vec4 => Value::Vec4(values),
        EffectClipParameterScrubKind::Range => Value::Range(aestra_bevy::ScalarRange {
            min: values[0],
            max: values[1],
        }),
    }
}

fn decorate_numeric_scrub_inputs(
    mut commands: Commands,
    children: Query<&Children>,
    inputs: Query<
        Entity,
        (
            Without<NumericScrubInput>,
            Or<(
                With<PropertiesNumberControl>,
                With<EmitterNumberControl>,
                With<EffectClipNumberControl>,
                With<EffectClipParameterNumberControl>,
                With<RendererNumberControl>,
                With<StartReferenceOffsetControl>,
                With<ChoreographyEventNumberControl>,
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
    properties_controls: Query<&PropertiesNumberControl>,
    emitter_controls: Query<&EmitterNumberControl>,
    effect_clip_controls: Query<&EffectClipNumberControl>,
    effect_clip_parameter_controls: Query<&EffectClipParameterNumberControl>,
    renderer_controls: Query<&RendererNumberControl>,
    start_reference_controls: Query<&StartReferenceOffsetControl>,
    choreography_event_controls: Query<&ChoreographyEventNumberControl>,
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
        &properties_controls,
        &emitter_controls,
        &effect_clip_controls,
        &effect_clip_parameter_controls,
        &renderer_controls,
        &start_reference_controls,
        &choreography_event_controls,
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
    properties_controls: &Query<&PropertiesNumberControl>,
    emitter_controls: &Query<&EmitterNumberControl>,
    effect_clip_controls: &Query<&EffectClipNumberControl>,
    effect_clip_parameter_controls: &Query<&EffectClipParameterNumberControl>,
    renderer_controls: &Query<&RendererNumberControl>,
    start_reference_controls: &Query<&StartReferenceOffsetControl>,
    choreography_event_controls: &Query<&ChoreographyEventNumberControl>,
) -> Option<(Entity, NumericScrubTarget)> {
    let mut candidate = entity;
    for _ in 0..4 {
        if let Ok(control) = properties_controls.get(candidate) {
            return Some((candidate, NumericScrubTarget::Properties(*control)));
        }
        if let Ok(control) = emitter_controls.get(candidate) {
            return Some((candidate, NumericScrubTarget::Emitter(*control)));
        }
        if let Ok(control) = effect_clip_controls.get(candidate) {
            return Some((candidate, NumericScrubTarget::EffectClip(*control)));
        }
        if let Ok(control) = effect_clip_parameter_controls.get(candidate)
            && let Some(control) = effect_clip_parameter_scrub_control(control)
        {
            return Some((candidate, NumericScrubTarget::EffectClipParameter(control)));
        }
        if let Ok(control) = renderer_controls.get(candidate) {
            return Some((candidate, NumericScrubTarget::Renderer(*control)));
        }
        if let Ok(control) = start_reference_controls.get(candidate) {
            return Some((
                candidate,
                NumericScrubTarget::StartReferenceOffset(*control),
            ));
        }
        if let Ok(control) = choreography_event_controls.get(candidate) {
            return Some((candidate, NumericScrubTarget::ChoreographyEvent(*control)));
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
        NumericScrubTarget::Properties(control) => control.step,
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
        NumericScrubTarget::EffectClipParameter(control) => {
            if control.kind == EffectClipParameterScrubKind::U32 {
                1.0
            } else {
                0.1
            }
        }
        NumericScrubTarget::Renderer(control) => renderer_number_step(control),
        NumericScrubTarget::StartReferenceOffset(_) => 0.05,
        NumericScrubTarget::ChoreographyEvent(ChoreographyEventNumberControl::Time(_)) => 0.05,
        NumericScrubTarget::ChoreographyEvent(ChoreographyEventNumberControl::Intensity(_)) => 0.1,
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
        NumericScrubTarget::Properties(control) => {
            properties_number_input_value(session, control).map(number_input_value_as_f32)
        }
        NumericScrubTarget::Emitter(control) => Some(emitter_number_input_value(session, control)),
        NumericScrubTarget::EffectClip(control) => effect_clip_number_input_value(session, control),
        NumericScrubTarget::EffectClipParameter(control) => {
            Some(control.values[control.component as usize])
        }
        NumericScrubTarget::Renderer(control) => renderer_number_input_value(session, control),
        NumericScrubTarget::StartReferenceOffset(control) => {
            start_reference(session, control.target).map(|reference| reference.offset)
        }
        NumericScrubTarget::ChoreographyEvent(control) => {
            let id = match control {
                ChoreographyEventNumberControl::Time(id)
                | ChoreographyEventNumberControl::Intensity(id) => id,
            };
            let event = session
                .effect
                .choreography_events
                .iter()
                .find(|event| event.id == id)?;
            match control {
                ChoreographyEventNumberControl::Time(_) => Some(event.time),
                ChoreographyEventNumberControl::Intensity(_) => match event.payload {
                    ChoreographyEventPayload::CameraShake { intensity } => Some(intensity),
                    _ => None,
                },
            }
        }
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
    if numeric_scrub_is_integer(target) {
        return (value.max(0.0).round().min(i32::MAX as f32) as i32).to_string();
    }
    let precision = numeric_scrub_precision(target, multiplier);
    crate::feathers::number_input::formatted(value, precision)
}

fn numeric_scrub_precision(target: NumericScrubTarget, multiplier: f32) -> usize {
    crate::feathers::number_input::decimal_places(numeric_scrub_step(target) * multiplier)
}

fn round_numeric_scrub_value(target: NumericScrubTarget, value: f32, multiplier: f32) -> f32 {
    if numeric_scrub_is_integer(target) {
        return value.round();
    }
    crate::feathers::number_input::rounded(value, numeric_scrub_precision(target, multiplier))
}

fn numeric_scrub_is_integer(target: NumericScrubTarget) -> bool {
    matches!(
        target,
        NumericScrubTarget::Properties(PropertiesNumberControl {
            kind: PropertiesNumberKind::U32,
            ..
        }) | NumericScrubTarget::EffectClipParameter(EffectClipParameterScrubControl {
            kind: EffectClipParameterScrubKind::U32,
            ..
        })
    )
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
        NumericScrubTarget::Properties(control) => {
            let mut value =
                clamp_properties_number(value, control.min, control.max).unwrap_or_default();
            if control.kind == PropertiesNumberKind::U32 {
                value = value.max(0.0).round();
            } else if control.kind == PropertiesNumberKind::Range
                && let Some(Value::Range(range)) =
                    properties_module_parameter(session, control.module, control.parameter)
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
        NumericScrubTarget::EffectClipParameter(control) => match control.kind {
            EffectClipParameterScrubKind::U32 => value.max(0.0).round(),
            EffectClipParameterScrubKind::Range if control.component == 0 => {
                value.min(control.values[1])
            }
            EffectClipParameterScrubKind::Range => value.max(control.values[0]),
            _ => value,
        },
        NumericScrubTarget::Renderer(RendererNumberControl::Softness(_)) => value.max(0.0),
        NumericScrubTarget::Renderer(RendererNumberControl::Uv(renderer, component)) => {
            normalize_renderer_uv_scrub_value(session, renderer, component, value)
        }
        NumericScrubTarget::Renderer(RendererNumberControl::FlipbookFrameRate(_)) => {
            value.clamp(1.0, 120.0)
        }
        NumericScrubTarget::StartReferenceOffset(control) => {
            normalize_start_reference_offset(session, control.target, value)
        }
        NumericScrubTarget::ChoreographyEvent(ChoreographyEventNumberControl::Time(_)) => {
            value.clamp(0.0, session.playback_duration())
        }
        NumericScrubTarget::ChoreographyEvent(ChoreographyEventNumberControl::Intensity(_)) => {
            value.max(0.0)
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
        NumericScrubTarget::Properties(control) => properties_module_parameter_command(
            session,
            control.module,
            control.parameter,
            updated_properties_number_value(session, control, value)?,
        ),
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
        NumericScrubTarget::EffectClipParameter(control) => {
            Some(EffectCommand::SetEffectClipParameterOverride {
                id: control.clip,
                parameter: control.parameter,
                value: effect_clip_parameter_scrub_value(control, value),
            })
        }
        NumericScrubTarget::Renderer(control) => {
            renderer_numeric_scrub_command(session, control, value)
        }
        NumericScrubTarget::StartReferenceOffset(control) => {
            let mut reference = start_reference(session, control.target)?;
            reference.offset = value;
            Some(match control.target {
                StartReferenceTarget::Emitter(id) => EffectCommand::SetEmitterStartReference {
                    id,
                    reference: Some(reference),
                },
                StartReferenceTarget::EffectClip(id) => {
                    EffectCommand::SetEffectClipStartReference {
                        id,
                        reference: Some(reference),
                    }
                }
                StartReferenceTarget::ChoreographyEvent(id) => {
                    EffectCommand::SetChoreographyEventTimeReference {
                        id,
                        reference: Some(reference),
                    }
                }
            })
        }
        NumericScrubTarget::ChoreographyEvent(ChoreographyEventNumberControl::Time(id)) => {
            Some(EffectCommand::SetChoreographyEventTime { id, time: value })
        }
        NumericScrubTarget::ChoreographyEvent(ChoreographyEventNumberControl::Intensity(id)) => {
            Some(EffectCommand::SetChoreographyEventPayload {
                id,
                payload: ChoreographyEventPayload::CameraShake { intensity: value },
            })
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
        NumericScrubTarget::Properties(control) => {
            apply_properties_number(session, control, value, localizer);
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
        NumericScrubTarget::EffectClipParameter(_) => {
            if let Some(command) = numeric_scrub_command(session, target, value) {
                session.execute(
                    localizer.text("properties-set-effect-clip-parameter-command"),
                    command,
                    true,
                );
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
        NumericScrubTarget::StartReferenceOffset(_) => {
            if let Some(command) = numeric_scrub_command(session, target, value) {
                session.execute(
                    localizer.text("properties-start-reference-command"),
                    command,
                    true,
                );
            }
        }
        NumericScrubTarget::ChoreographyEvent(control) => {
            if let Some(command) = numeric_scrub_command(session, target, value) {
                let label = match control {
                    ChoreographyEventNumberControl::Time(_) => "Moved choreography event",
                    ChoreographyEventNumberControl::Intensity(_) => {
                        "Changed camera shake intensity"
                    }
                };
                session.execute(label, command, true);
            }
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
        NumericScrubTarget::Properties(control) => format!("Changed {}", control.parameter),
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
        NumericScrubTarget::EffectClipParameter(_) => "Changed instance parameter".into(),
        NumericScrubTarget::StartReferenceOffset(_) => "Changed marker offset".into(),
        NumericScrubTarget::ChoreographyEvent(ChoreographyEventNumberControl::Time(_)) => {
            "Moved choreography event".into()
        }
        NumericScrubTarget::ChoreographyEvent(ChoreographyEventNumberControl::Intensity(_)) => {
            "Changed camera shake intensity".into()
        }
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

fn handle_properties_integer_change(
    change: On<ValueChange<i32>>,
    controls: Query<&PropertiesNumberControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if control.kind != PropertiesNumberKind::U32 {
        return;
    }
    apply_properties_number(&mut session, *control, change.value as f32, &localizer);
}

fn handle_properties_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&PropertiesNumberControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if control.kind == PropertiesNumberKind::U32 {
        return;
    }
    apply_properties_number(&mut session, *control, change.value, &localizer);
}

fn properties_module_parameter(
    session: &EditorSession,
    module: ModuleId,
    parameter: &str,
) -> Option<Value> {
    let module = session
        .selected_layer()
        .modules
        .iter()
        .find(|candidate| candidate.id == module)?;
    if let Some(parameter_id) = module.bindings.get(parameter) {
        return session
            .effect
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter_id)
            .map(|parameter| parameter.default.clone());
    }
    module_parameter(module, parameter)
}

fn properties_module_parameter_command(
    session: &EditorSession,
    module: ModuleId,
    input: &str,
    value: Value,
) -> Option<EffectCommand> {
    let module_instance = session
        .selected_layer()
        .modules
        .iter()
        .find(|candidate| candidate.id == module)?;
    if let Some(parameter_id) = module_instance.bindings.get(input) {
        let mut parameter = session
            .effect
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter_id)?
            .clone();
        parameter.default = value;
        return Some(EffectCommand::SetParameter {
            id: *parameter_id,
            parameter,
        });
    }
    if let Some(source) = module_instance.property_source(input)
        && source != PropertySourceKind::Constant
        && module_instance
            .property_source_values
            .get(input)
            .is_some_and(|values| values.iter().any(|value| value.source == source))
    {
        return Some(EffectCommand::SetModulePropertySourceValue {
            emitter: session.selected_layer().id,
            module,
            parameter: input.to_owned(),
            source,
            value,
        });
    }
    Some(EffectCommand::SetModuleParameter {
        emitter: session.selected_layer().id,
        module,
        parameter: input.to_owned(),
        value,
    })
}

fn apply_properties_number(
    session: &mut EditorSession,
    control: PropertiesNumberControl,
    raw_value: f32,
    localizer: &Localizer,
) -> bool {
    let Some(value) = clamp_properties_number(raw_value, control.min, control.max) else {
        set_properties_status(
            session,
            localizer,
            PropertiesStatus::FiniteNumberRequired(control.parameter.into()),
        );
        return false;
    };
    let Some(current) = properties_module_parameter(session, control.module, control.parameter)
    else {
        set_properties_status(session, localizer, PropertiesStatus::TargetUnavailable);
        return false;
    };
    let Some(updated) = updated_properties_number_value(session, control, value) else {
        set_properties_status(
            session,
            localizer,
            PropertiesStatus::IncompatibleMetadata(control.parameter.into()),
        );
        return false;
    };
    if updated == current {
        return false;
    }
    let is_bound = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == control.module)
        .is_some_and(|module| module.bindings.contains_key(control.parameter));
    if !is_bound {
        session.set_module_parameter(control.module, control.parameter, updated);
        return true;
    }
    let Some(command) =
        properties_module_parameter_command(session, control.module, control.parameter, updated)
    else {
        return false;
    };
    session.execute(
        localizer.text("properties-edit-module-input-command"),
        command,
        true,
    )
}

fn updated_properties_number_value(
    session: &EditorSession,
    control: PropertiesNumberControl,
    raw_value: f32,
) -> Option<Value> {
    let value = clamp_properties_number(raw_value, control.min, control.max)?;
    let current = properties_module_parameter(session, control.module, control.parameter)?;
    match (control.kind, current) {
        (PropertiesNumberKind::U32, Value::U32(_)) => Some(Value::U32(
            value.max(0.0).round().min(u32::MAX as f32) as u32,
        )),
        (PropertiesNumberKind::Scalar, Value::Scalar(_)) => Some(Value::Scalar(value)),
        (PropertiesNumberKind::CurveConstant, Value::Curve(mut curve)) => {
            let key = curve.keys.first_mut()?;
            key.value = value;
            curve.keys.truncate(1);
            Some(Value::Curve(curve))
        }
        (PropertiesNumberKind::Vector, Value::Vec2(mut vector)) => {
            let component = vector.get_mut(control.component as usize)?;
            *component = value;
            Some(Value::Vec2(vector))
        }
        (PropertiesNumberKind::Vector, Value::Vec3(mut vector)) => {
            let component = vector.get_mut(control.component as usize)?;
            *component = value;
            Some(Value::Vec3(vector))
        }
        (PropertiesNumberKind::Vector, Value::Vec4(mut vector)) => {
            let component = vector.get_mut(control.component as usize)?;
            *component = value;
            Some(Value::Vec4(vector))
        }
        (PropertiesNumberKind::Range, Value::Range(mut range)) => {
            if control.component == 0 {
                range.min = value.min(range.max);
            } else {
                range.max = value.max(range.min);
            }
            Some(Value::Range(range))
        }
        (PropertiesNumberKind::RangeConstant, Value::Range(_)) => {
            Some(Value::Range(ScalarRange::new(value, value)))
        }
        (PropertiesNumberKind::Shape, Value::Shape(shape)) => {
            shape_with_dimension(shape, control.component, value).map(Value::Shape)
        }
        _ => None,
    }
}

fn clamp_properties_number(value: f32, min: Option<f32>, max: Option<f32>) -> Option<f32> {
    if !value.is_finite() {
        return None;
    }
    let value = min.map_or(value, |min| value.max(min));
    Some(max.map_or(value, |max| value.min(max)))
}

fn spawn_read_only_properties_shell(
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
                &localizer.text("properties-referenced-effect-heading"),
                &localizer.text(if instance_editable {
                    "properties-instance-editable"
                } else {
                    "properties-read-only"
                }),
            );
            panel.spawn((
                Text::new(title),
                PropertiesTitle,
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
                        ScrollMemoryKey::Properties,
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
struct EffectClipPropertiesTimingText {
    clip: EffectClipId,
    field: EffectClipPropertiesTimingField,
}

#[derive(Clone, Copy)]
enum EffectClipPropertiesTimingField {
    Start,
    SourceOffset,
    Duration,
}

fn sync_effect_clip_properties_timing(
    session: Res<EditorSession>,
    timeline: Option<Res<TimelineState>>,
    mut texts: Query<(&EffectClipPropertiesTimingText, &mut Text)>,
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
            EffectClipPropertiesTimingField::Start => start,
            EffectClipPropertiesTimingField::SourceOffset => source_offset,
            EffectClipPropertiesTimingField::Duration => duration,
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

fn effect_clip_breadcrumbs(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    path: &EffectClipPath,
) -> Vec<(String, Option<DocumentAction>)> {
    let mut breadcrumbs = vec![(session.effect.name.clone(), None)];
    let mut effect = session.effect.clone();
    for id in path.ids() {
        let Some(clip) = effect.effect_clips.iter().find(|clip| clip.id == *id) else {
            break;
        };
        breadcrumbs.push((
            effect_clip_catalog_name(catalog, clip.source),
            Some(DocumentAction::OpenSource(clip.source)),
        ));
        let Ok(source) = catalog.load_effect(clip.source) else {
            break;
        };
        effect = source;
    }
    breadcrumbs
}

fn spawn_source_navigation_row(
    parent: &mut ChildSpawnerCommands,
    breadcrumbs: &[(String, Option<DocumentAction>)],
    trailing_action: Option<(DocumentAction, &str)>,
    explode_clip: Option<(EffectClipId, &str)>,
    asset_server: &AssetServer,
) {
    let items = breadcrumbs
        .iter()
        .map(|(label, action)| BreadcrumbItem {
            label: label.clone(),
            action: *action,
        })
        .collect::<Vec<_>>();
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        })
        .insert(BorderColor::all(theme::BORDER.with_alpha(0.65)))
        .with_children(|row| {
            spawn_breadcrumb(
                row,
                &items,
                BreadcrumbProps {
                    height: 28.0,
                    font: fonts::REGULAR,
                    font_size: 9.0,
                    text_offset_y: 0.0,
                    uppercase: false,
                    flex_grow: 1.0,
                    max_ancestor_width: 180.0,
                    max_current_width: 180.0,
                    ancestor_color: theme::TEXT,
                    current_color: theme::ACCENT,
                    compact_ancestors: false,
                    overflow_label: "",
                    current_tooltip: None,
                    ancestor_tooltips: false,
                },
                asset_server,
            );
            if let Some((action, label)) = trailing_action {
                spawn_feathers_action_button(row, label, action, false);
            }
            if let Some((clip, label)) = explode_clip {
                spawn_feathers_action_button(
                    row,
                    label,
                    crate::library::LibraryAction::ExplodeEffectClip(clip),
                    false,
                );
            }
        });
}

fn spawn_edit_source_navigation(
    parent: &mut ChildSpawnerCommands,
    breadcrumbs: &[(String, Option<DocumentAction>)],
    source: EffectAssetRef,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    asset_server: &AssetServer,
    explode_clip: Option<EffectClipId>,
) {
    if catalog.openable_path(source).is_some() {
        spawn_source_navigation_row(
            parent,
            breadcrumbs,
            Some((
                DocumentAction::OpenSource(source),
                &localizer.text("properties-edit-source"),
            )),
            explode_clip
                .map(|clip| (clip, localizer.text("properties-explode-effect-clip")))
                .as_ref()
                .map(|(clip, label)| (*clip, label.as_str())),
            asset_server,
        );
    }
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
        localizer.text("properties-repair-reference"),
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
                    &localizer.text("properties-repair-search-placeholder"),
                    &localizer.text("properties-repair-search-clear"),
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
                    localizer.text("properties-repair-reference"),
                    entry.display_name
                );
                let row = spawn_action_list_row(
                    card,
                    &entry.display_name,
                    Some(&path),
                    None,
                    &accessible,
                    PropertiesAction::RepairEffectClipSource {
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
                &localizer.text("properties-repair-no-results-title"),
                &localizer.text("properties-repair-no-results-message"),
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

#[derive(Debug, Clone, PartialEq)]
struct EffectClipParameterEntry {
    id: ParameterId,
    name: String,
    value: Value,
    overridden: bool,
    issue: Option<EffectClipParameterIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectClipParameterIssue {
    Missing,
    Hidden,
    TypeMismatch {
        expected: ValueType,
        actual: ValueType,
    },
    SourceUnavailable,
}

fn effect_clip_parameter_entries(
    clip: &EffectClip,
    source: Option<&EffectAsset>,
) -> Vec<EffectClipParameterEntry> {
    let mut entries = Vec::new();
    if let Some(source) = source {
        for parameter in source
            .parameters
            .iter()
            .filter(|parameter| parameter.exposed)
        {
            let authored = clip.parameter_overrides.get(&parameter.id);
            let issue = authored.and_then(|value| {
                (value.value_type() != parameter.default.value_type()).then_some(
                    EffectClipParameterIssue::TypeMismatch {
                        expected: parameter.default.value_type(),
                        actual: value.value_type(),
                    },
                )
            });
            entries.push(EffectClipParameterEntry {
                id: parameter.id,
                name: parameter.name.clone(),
                value: authored
                    .cloned()
                    .unwrap_or_else(|| parameter.default.clone()),
                overridden: authored.is_some(),
                issue,
            });
        }
        for (&id, value) in &clip.parameter_overrides {
            let parameter = source
                .parameters
                .iter()
                .find(|parameter| parameter.id == id);
            if parameter.is_some_and(|parameter| parameter.exposed) {
                continue;
            }
            entries.push(EffectClipParameterEntry {
                id,
                name: parameter.map_or_else(|| id.to_string(), |parameter| parameter.name.clone()),
                value: value.clone(),
                overridden: true,
                issue: Some(if parameter.is_some() {
                    EffectClipParameterIssue::Hidden
                } else {
                    EffectClipParameterIssue::Missing
                }),
            });
        }
    } else {
        entries.extend(clip.parameter_overrides.iter().map(|(&id, value)| {
            EffectClipParameterEntry {
                id,
                name: id.to_string(),
                value: value.clone(),
                overridden: true,
                issue: Some(EffectClipParameterIssue::SourceUnavailable),
            }
        }));
    }
    entries
}

fn spawn_effect_clip_instance_parameters(
    parent: &mut ChildSpawnerCommands,
    clip: &EffectClip,
    source: Option<&EffectAsset>,
    localizer: &Localizer,
) {
    let entries = effect_clip_parameter_entries(clip, source);
    spawn_read_only_card(
        parent,
        localizer.text("properties-instance-parameters"),
        |card| {
            if entries.is_empty() {
                card.spawn_empty().apply_scene(label_dim(
                    localizer.text("properties-instance-parameters-empty"),
                ));
                return;
            }
            for entry in &entries {
                spawn_effect_clip_parameter_row(card, clip.id, entry, localizer);
            }
        },
    );
}

fn spawn_effect_clip_parameter_row(
    parent: &mut ChildSpawnerCommands,
    clip: EffectClipId,
    entry: &EffectClipParameterEntry,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            padding: UiRect::vertical(Val::Px(3.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|column| {
            column
                .spawn(Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(27.0),
                    min_width: Val::Px(0.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(5.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new(&entry.name),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Node {
                            width: Val::Px(92.0),
                            flex_shrink: 0.0,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                    ));
                    row.spawn(Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        column_gap: Val::Px(4.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|controls| {
                        if entry.issue.is_none() {
                            spawn_effect_clip_parameter_control(
                                controls,
                                clip,
                                entry.id,
                                &entry.name,
                                &entry.value,
                            );
                        } else {
                            controls
                                .spawn_empty()
                                .apply_scene(label_dim(format_value(entry.value.clone())));
                        }
                    });
                    if entry.overridden {
                        row.spawn((
                            Text::new(localizer.text("properties-parameter-overridden")),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::ACCENT),
                            EffectClipParameterOverrideIndicator(entry.id),
                            Pickable::IGNORE,
                        ));
                        let reset = mini_button(
                            row,
                            "↺",
                            PropertiesAction::ResetEffectClipParameter {
                                clip,
                                parameter: entry.id,
                            },
                        );
                        row.commands().entity(reset).insert((
                            EditorTooltip::description(
                                localizer.text("properties-reset-to-source"),
                            ),
                            AccessibleLabel(localizer.text("properties-reset-to-source")),
                        ));
                    }
                });
            if let Some(issue) = entry.issue {
                column.spawn((
                    Text::new(effect_clip_parameter_issue_text(localizer, issue)),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.94, 0.55, 0.27)),
                    EffectClipParameterDiagnostic(entry.id),
                    Pickable::IGNORE,
                ));
            }
        });
}

fn spawn_effect_clip_parameter_control(
    parent: &mut ChildSpawnerCommands,
    clip: EffectClipId,
    parameter: ParameterId,
    name: &str,
    value: &Value,
) {
    match value {
        Value::Bool(value) => {
            let mut checkbox = parent.spawn_empty();
            checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                EffectClipParameterToggleControl {
                    clip,
                    parameter,
                    value: *value,
                },
                AccessibleLabel(name.to_owned()),
            ));
            if *value {
                checkbox.insert(Checked);
            }
        }
        Value::U32(_) => {
            parent
                .spawn_empty()
                .apply_scene(ui_shell::feathers_integer_input())
                .insert((
                    EffectClipParameterNumberControl {
                        clip,
                        parameter,
                        value: value.clone(),
                        component: 0,
                    },
                    AccessibleLabel(name.to_owned()),
                ));
        }
        Value::Scalar(_) => {
            spawn_effect_clip_parameter_scalar_input(parent, clip, parameter, name, value, "", 0)
        }
        Value::Vec2(_) => {
            for (axis, component) in [("X", 0), ("Y", 1)] {
                spawn_effect_clip_parameter_scalar_input(
                    parent, clip, parameter, name, value, axis, component,
                );
            }
        }
        Value::Vec3(_) => {
            for (axis, component) in [("X", 0), ("Y", 1), ("Z", 2)] {
                spawn_effect_clip_parameter_scalar_input(
                    parent, clip, parameter, name, value, axis, component,
                );
            }
        }
        Value::Vec4(_) => {
            for (axis, component) in [("X", 0), ("Y", 1), ("Z", 2), ("W", 3)] {
                spawn_effect_clip_parameter_scalar_input(
                    parent, clip, parameter, name, value, axis, component,
                );
            }
        }
        Value::Range(_) => {
            for (axis, component) in [("MIN", 0), ("MAX", 1)] {
                spawn_effect_clip_parameter_scalar_input(
                    parent, clip, parameter, name, value, axis, component,
                );
            }
        }
        Value::Text(value) => {
            spawn_text_input(
                parent,
                value,
                name,
                EffectClipParameterTextControl {
                    clip,
                    parameter,
                    value: value.clone(),
                },
            );
        }
        _ => {
            parent
                .spawn_empty()
                .apply_scene(label_dim(format_value(value.clone())));
        }
    }
}

fn spawn_effect_clip_parameter_scalar_input(
    parent: &mut ChildSpawnerCommands,
    clip: EffectClipId,
    parameter: ParameterId,
    name: &str,
    value: &Value,
    axis: &'static str,
    component: u8,
) {
    parent
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
                let color = match axis {
                    "X" | "MIN" => tokens::TEXT_INPUT_X_AXIS,
                    "Y" | "MAX" => tokens::TEXT_INPUT_Y_AXIS,
                    "Z" => tokens::TEXT_INPUT_Z_AXIS,
                    _ => tokens::TEXT_INPUT_BG,
                };
                input.apply_scene(ui_shell::feathers_labeled_scalar_input(axis, color));
            }
            input.insert((
                EffectClipParameterNumberControl {
                    clip,
                    parameter,
                    value: value.clone(),
                    component,
                },
                AccessibleLabel(if axis.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name} {axis}")
                }),
            ));
        });
}

fn effect_clip_parameter_issue_text(
    localizer: &Localizer,
    issue: EffectClipParameterIssue,
) -> String {
    match issue {
        EffectClipParameterIssue::Missing => {
            localizer.text("properties-parameter-override-missing")
        }
        EffectClipParameterIssue::Hidden => localizer.text("properties-parameter-override-hidden"),
        EffectClipParameterIssue::TypeMismatch { expected, actual } => {
            let mut args = FluentArgs::new();
            args.set("expected", format!("{expected:?}"));
            args.set("actual", format!("{actual:?}"));
            localizer.text_with("properties-parameter-override-type-mismatch", &args)
        }
        EffectClipParameterIssue::SourceUnavailable => {
            localizer.text("properties-parameter-override-source-unavailable")
        }
    }
}

fn spawn_effect_clip_properties(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    repair: &EffectClipRepairState,
    localizer: &Localizer,
    id: EffectClipId,
    asset_server: &AssetServer,
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
    spawn_read_only_properties_shell(parent, &source_name, localizer, true, |stack| {
        spawn_edit_source_navigation(
            stack,
            &[
                (session.effect.name.clone(), None),
                (
                    source_name.clone(),
                    Some(DocumentAction::OpenSource(clip.source)),
                ),
            ],
            clip.source,
            catalog,
            localizer,
            asset_server,
            Some(clip.id),
        );
        spawn_read_only_card(stack, localizer.text("properties-effect-clip"), |card| {
            spawn_read_only_row(card, localizer.text("properties-source"), &source_name);
            spawn_start_reference_controls(
                card,
                session,
                StartReferenceTarget::EffectClip(clip.id),
                localizer,
            );
            let start = spawn_read_only_row(
                card,
                localizer.text("properties-start"),
                format!("{:.3} s", clip.start_time),
            );
            card.commands()
                .entity(start)
                .insert(EffectClipPropertiesTimingText {
                    clip: clip.id,
                    field: EffectClipPropertiesTimingField::Start,
                });
            let source_offset = spawn_read_only_row(
                card,
                localizer.text("properties-source-offset"),
                format!("{:.3} s", clip.source_offset),
            );
            card.commands()
                .entity(source_offset)
                .insert(EffectClipPropertiesTimingText {
                    clip: clip.id,
                    field: EffectClipPropertiesTimingField::SourceOffset,
                });
            let duration = spawn_read_only_row(
                card,
                localizer.text("properties-duration"),
                format!("{:.3} s", clip.duration),
            );
            card.commands()
                .entity(duration)
                .insert(EffectClipPropertiesTimingText {
                    clip: clip.id,
                    field: EffectClipPropertiesTimingField::Duration,
                });
            spawn_read_only_row(
                card,
                localizer.text("properties-seed"),
                format!("{:?}", clip.seed),
            );
        });
        spawn_effect_clip_transform_controls(stack, clip.id);
        spawn_effect_clip_instance_parameters(stack, clip, source.as_ref(), localizer);
        if let Some(error) = dependency_error.as_deref() {
            spawn_effect_clip_repair(stack, session, catalog, repair, localizer, clip, error);
        }
        spawn_read_only_card(stack, localizer.text("properties-source-summary"), |card| {
            if let Some(source) = &source {
                spawn_read_only_row(card, localizer.text("properties-name"), &source.name);
                spawn_read_only_row(
                    card,
                    localizer.text("properties-duration"),
                    format!("{:.3} s", source.duration),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-emitters"),
                    source.emitters.len().to_string(),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-looping"),
                    source.looping.to_string(),
                );
            } else {
                spawn_read_only_row(
                    card,
                    localizer.text("properties-status"),
                    localizer.text("properties-source-unavailable"),
                );
            }
        });
        stack.spawn((
            Text::new(localizer.text("properties-effect-clip-read-only-description")),
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

fn spawn_referenced_emitter_properties(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    path: &EffectClipPath,
    selected_emitter: EmitterId,
    asset_server: &AssetServer,
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
    spawn_read_only_properties_shell(parent, &emitter.name, localizer, false, |stack| {
        let mut breadcrumbs = effect_clip_breadcrumbs(session, catalog, path);
        breadcrumbs.push((emitter.name.clone(), None));
        spawn_edit_source_navigation(
            stack,
            &breadcrumbs,
            clip.source,
            catalog,
            localizer,
            asset_server,
            None,
        );
        spawn_read_only_card(stack, localizer.text("properties-reference"), |card| {
            spawn_read_only_row(card, localizer.text("properties-source"), &source_name);
            spawn_read_only_row(card, localizer.text("properties-emitter"), &emitter.name);
            spawn_read_only_row(
                card,
                localizer.text("properties-mode"),
                localizer.text("properties-read-only"),
            );
        });
        spawn_read_only_card(stack, localizer.text("properties-emitter"), |card| {
            spawn_read_only_row(
                card,
                localizer.text("properties-enabled"),
                emitter.enabled.to_string(),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-capacity"),
                emitter.max_particles.to_string(),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-start"),
                format!("{:.3} s", emitter.start_time),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-duration"),
                format!("{:.3} s", emitter.duration),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-domain"),
                format!("{:?}", emitter.simulation_domain),
            );
        });
        spawn_read_only_card(stack, localizer.text("properties-transform"), |card| {
            spawn_read_only_row(
                card,
                localizer.text("properties-position"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    emitter.transform.translation[0],
                    emitter.transform.translation[1],
                    emitter.transform.translation[2]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-rotation"),
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
                localizer.text("properties-scale"),
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
                    localizer.text("properties-stage"),
                    format!("{:?}", module.stage),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-enabled"),
                    module.enabled.to_string(),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-parameters"),
                    format!("{:?}", module.parameters),
                );
            });
        }
        for renderer in &emitter.renderers {
            spawn_read_only_card(stack, &renderer.renderer_type.0, |card| {
                spawn_read_only_row(
                    card,
                    localizer.text("properties-enabled"),
                    renderer.enabled.to_string(),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-section-properties"),
                    format!("{:?}", renderer.properties),
                );
            });
        }
    });
    true
}

fn spawn_referenced_effect_clip_properties(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    path: &EffectClipPath,
    asset_server: &AssetServer,
) -> bool {
    let Some((clip, source)) = resolve_effect_clip_path(session, catalog, path) else {
        return false;
    };
    let source_name = effect_clip_catalog_name(catalog, clip.source);
    spawn_read_only_properties_shell(parent, &source_name, localizer, false, |stack| {
        spawn_edit_source_navigation(
            stack,
            &effect_clip_breadcrumbs(session, catalog, path),
            clip.source,
            catalog,
            localizer,
            asset_server,
            None,
        );
        spawn_read_only_card(stack, localizer.text("properties-effect-clip"), |card| {
            spawn_read_only_row(card, localizer.text("properties-source"), &source_name);
            spawn_read_only_row(
                card,
                localizer.text("properties-start"),
                format!("{:.3} s", clip.start_time),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-source-offset"),
                format!("{:.3} s", clip.source_offset),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-duration"),
                format!("{:.3} s", clip.duration),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-seed"),
                format!("{:?}", clip.seed),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-mode"),
                localizer.text("properties-read-only"),
            );
        });
        spawn_read_only_card(stack, localizer.text("properties-transform"), |card| {
            spawn_read_only_row(
                card,
                localizer.text("properties-position"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    clip.transform.translation[0],
                    clip.transform.translation[1],
                    clip.transform.translation[2]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-rotation"),
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
                localizer.text("properties-scale"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    clip.transform.scale[0], clip.transform.scale[1], clip.transform.scale[2]
                ),
            );
        });
        spawn_read_only_card(stack, localizer.text("properties-source-summary"), |card| {
            spawn_read_only_row(card, localizer.text("properties-name"), &source.name);
            spawn_read_only_row(
                card,
                localizer.text("properties-duration"),
                format!("{:.3} s", source.duration),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-emitters"),
                source.emitters.len().to_string(),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-looping"),
                source.looping.to_string(),
            );
        });
        stack.spawn((
            Text::new(localizer.text("properties-effect-clip-read-only-description")),
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

pub(crate) fn spawn_properties(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    palette: &ModulePaletteState,
    localizer: &Localizer,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    timeline: &TimelineState,
    repair: &EffectClipRepairState,
    navigation: Option<&SourceNavigationState>,
    asset_server: &AssetServer,
) {
    if let Some(navigation) = navigation.filter(|navigation| navigation.can_go_back()) {
        let depth = navigation.depth();
        let mut breadcrumbs = navigation
            .breadcrumb(&session.effect.name)
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                (
                    name,
                    (index < depth).then_some(DocumentAction::NavigateSourceAncestor(index)),
                )
            })
            .collect::<Vec<_>>();
        if timeline.inspected_child.is_none()
            && let Some(emitter) = session.selection.emitter(&session.effect)
            && let Some(emitter) = session
                .effect
                .emitters
                .iter()
                .find(|candidate| candidate.id == emitter)
        {
            breadcrumbs.push((emitter.name.clone(), None));
        }
        spawn_source_navigation_row(parent, &breadcrumbs, None, None, asset_server);
    }
    if let Some(selection) = timeline.inspected_child.as_ref() {
        let spawned = match selection {
            EffectClipChildSelection::EffectClip { path } => {
                spawn_referenced_effect_clip_properties(
                    parent,
                    session,
                    catalog,
                    localizer,
                    path,
                    asset_server,
                )
            }
            EffectClipChildSelection::Emitter { path, emitter } => {
                spawn_referenced_emitter_properties(
                    parent,
                    session,
                    catalog,
                    localizer,
                    path,
                    *emitter,
                    asset_server,
                )
            }
        };
        if spawned {
            return;
        }
    }
    if let SemanticTarget::Marker(marker) = session.selection.primary
        && spawn_marker_properties(parent, session, localizer, marker)
    {
        return;
    }
    if let SemanticTarget::ChoreographyEvent(event) = session.selection.primary
        && spawn_choreography_event_properties(parent, session, localizer, event)
    {
        return;
    }
    if let SemanticTarget::EffectClip(clip) = session.selection.primary
        && spawn_effect_clip_properties(
            parent,
            session,
            catalog,
            repair,
            localizer,
            clip,
            asset_server,
        )
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
                PropertiesTitle,
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
                        ScrollMemoryKey::Properties,
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
                    spawn_emitter_transform_controls(stack);
                    spawn_emitter_timing_controls(stack, session, localizer);
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
                                    properties_renderer_collapsed(settings, renderer),
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
                                properties_module_collapsed(settings, module),
                                asset_server,
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

fn spawn_marker_properties(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
    id: MarkerId,
) -> bool {
    let Some(marker) = session.effect.markers.iter().find(|marker| marker.id == id) else {
        return false;
    };
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel_heading(panel, "TIMELINE MARKER", "EDITABLE");
            panel.spawn((
                Text::new(&marker.name),
                PropertiesTitle,
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
                    padding: UiRect::all(Val::Px(8.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(7.0),
                    ..default()
                })
                .with_children(|stack| {
                    spawn_read_only_card(stack, localizer.text("properties-marker"), |card| {
                        spawn_text_field(
                            card,
                            &localizer.text("properties-name"),
                            &localizer.text("properties-marker-name-description"),
                            &marker.name,
                            DocumentTextControl::Marker(id),
                        );
                        crate::feathers::field_row::spawn_field_row(
                            card,
                            crate::feathers::field_row::FieldRowProps::new(
                                localizer.text("properties-time"),
                            )
                            .with_control_min_width(150.0),
                            EditorTooltip::description(
                                localizer.text("properties-marker-time-description"),
                            ),
                            |controls| {
                                controls
                                    .spawn_empty()
                                    .apply_scene(ui_shell::feathers_scalar_input())
                                    .insert((
                                        MarkerNumberControl(id),
                                        AccessibleLabel(localizer.text("properties-time")),
                                    ));
                            },
                        );
                        mini_button(
                            card,
                            &localizer.text("properties-marker-delete"),
                            PropertiesAction::DeleteMarker(id),
                        );
                    });
                });
        });
    true
}

fn spawn_choreography_event_properties(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
    id: ChoreographyEventId,
) -> bool {
    let Some(event) = session
        .effect
        .choreography_events
        .iter()
        .find(|event| event.id == id)
    else {
        return false;
    };
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel_heading(panel, "CHOREOGRAPHY EVENT", "EDITABLE");
            panel.spawn((
                Text::new(&event.name),
                PropertiesTitle,
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
                    padding: UiRect::all(Val::Px(8.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(7.0),
                    ..default()
                })
                .with_children(|stack| {
                    spawn_read_only_card(
                        stack,
                        localizer.text("properties-choreography-event"),
                        |card| {
                            spawn_text_field(
                                card,
                                &localizer.text("properties-name"),
                                &localizer.text("properties-choreography-event-name-description"),
                                &event.name,
                                DocumentTextControl::ChoreographyEvent(id),
                            );
                            crate::feathers::field_row::spawn_field_row(
                                card,
                                crate::feathers::field_row::FieldRowProps::new(
                                    localizer.text("properties-time"),
                                )
                                .with_control_min_width(150.0),
                                EditorTooltip::description(
                                    localizer
                                        .text("properties-choreography-event-time-description"),
                                ),
                                |controls| {
                                    controls
                                        .spawn_empty()
                                        .apply_scene(ui_shell::feathers_scalar_input())
                                        .insert((
                                            ChoreographyEventNumberControl::Time(id),
                                            AccessibleLabel(localizer.text("properties-time")),
                                        ));
                                },
                            );
                            spawn_start_reference_controls(
                                card,
                                session,
                                StartReferenceTarget::ChoreographyEvent(id),
                                localizer,
                            );
                            let kinds = [
                                ChoreographyEventKind::GameplayNotify,
                                ChoreographyEventKind::PlaySound,
                                ChoreographyEventKind::CameraShake,
                                ChoreographyEventKind::SpawnChildEffect,
                            ];
                            let kind_options = kinds
                                .into_iter()
                                .map(|kind| ComboOption {
                                    label: choreography_event_kind_label(localizer, kind),
                                    selected: event.payload.kind() == kind,
                                    action: PropertiesAction::SetChoreographyEventKind { id, kind },
                                })
                                .collect::<Vec<_>>();
                            spawn_properties_combo_row(
                                card,
                                &localizer.text("properties-choreography-event-type"),
                                &choreography_event_kind_label(localizer, event.payload.kind()),
                                &kind_options,
                                Some(
                                    &localizer
                                        .text("properties-choreography-event-type-description"),
                                ),
                            );
                            spawn_choreography_event_payload_control(card, event, localizer);
                            mini_button(
                                card,
                                &localizer.text("properties-choreography-event-delete"),
                                PropertiesAction::DeleteChoreographyEvent(id),
                            );
                        },
                    );
                });
        });
    true
}

fn choreography_event_kind_label(localizer: &Localizer, kind: ChoreographyEventKind) -> String {
    localizer.text(match kind {
        ChoreographyEventKind::GameplayNotify => "properties-event-kind-gameplay-notify",
        ChoreographyEventKind::PlaySound => "properties-event-kind-play-sound",
        ChoreographyEventKind::CameraShake => "properties-event-kind-camera-shake",
        ChoreographyEventKind::SpawnChildEffect => "properties-event-kind-spawn-child-effect",
    })
}

fn spawn_choreography_event_payload_control(
    parent: &mut ChildSpawnerCommands,
    event: &aestra_bevy::ChoreographyEvent,
    localizer: &Localizer,
) {
    match &event.payload {
        ChoreographyEventPayload::CameraShake { .. } => {
            crate::feathers::field_row::spawn_field_row(
                parent,
                crate::feathers::field_row::FieldRowProps::new(
                    localizer.text("properties-event-intensity"),
                )
                .with_control_min_width(150.0),
                EditorTooltip::description(
                    localizer.text("properties-event-intensity-description"),
                ),
                |controls| {
                    controls
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((
                            ChoreographyEventNumberControl::Intensity(event.id),
                            AccessibleLabel(localizer.text("properties-event-intensity")),
                        ));
                },
            );
        }
        payload => {
            let (label_key, value) = match payload {
                ChoreographyEventPayload::GameplayNotify { topic } => {
                    ("properties-event-topic", topic.as_str())
                }
                ChoreographyEventPayload::PlaySound { cue } => {
                    ("properties-event-cue", cue.as_str())
                }
                ChoreographyEventPayload::SpawnChildEffect { effect } => {
                    ("properties-event-effect", effect.as_str())
                }
                ChoreographyEventPayload::CameraShake { .. } => unreachable!(),
            };
            let label = localizer.text(label_key);
            crate::feathers::field_row::spawn_field_row(
                parent,
                crate::feathers::field_row::FieldRowProps::new(&label)
                    .with_control_min_width(150.0),
                EditorTooltip::description(
                    localizer.text("properties-choreography-event-payload-description"),
                ),
                |inputs| {
                    spawn_text_input(
                        inputs,
                        value,
                        &label,
                        ChoreographyEventPayloadTextControl(event.id),
                    );
                },
            );
        }
    }
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
                Text::new(localizer.text("properties-effect")),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            spawn_text_field(
                card,
                &localizer.text("properties-effect-name"),
                &localizer.text("properties-effect-name-description"),
                &session.effect.name,
                DocumentTextControl::Effect,
            );
        });

    let emitter = session.selected_layer();
    parent
        .spawn((
            PropertiesSemanticTarget {
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
                Text::new(localizer.text("properties-emitter")),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            spawn_text_field(
                card,
                &localizer.text("properties-emitter-name"),
                &localizer.text("properties-emitter-name-description"),
                &emitter.name,
                DocumentTextControl::Emitter,
            );
            spawn_document_toggle(
                card,
                &localizer.text("properties-emitter-enabled"),
                &localizer.text("properties-emitter-enabled-description"),
                emitter.enabled,
                DocumentToggleControl::EmitterEnabled,
            );
            crate::feathers::field_row::spawn_field_row(
                card,
                crate::feathers::field_row::FieldRowProps::new(
                    localizer.text("properties-emitter-capacity"),
                )
                .with_control_min_width(150.0),
                EditorTooltip::description(
                    localizer.text("properties-emitter-capacity-description"),
                ),
                |controls| {
                    controls
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_integer_input())
                        .insert((
                            EmitterCapacityControl,
                            AccessibleLabel(localizer.text("properties-emitter-capacity")),
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
        Text::new(localizer.text("properties-events")),
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
            .apply_scene(label_dim(localizer.text("properties-events-empty")));
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
                PropertiesSemanticTarget {
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
                    Text::new(localizer.text_with("properties-event-link", &args)),
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
                mini_button(row, "×", PropertiesAction::DeleteEventLink(event.id));
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
                label: localizer.text_with("properties-event-link", &args),
                selected: false,
                action: PropertiesAction::AddEventLink {
                    trigger,
                    target: target.id,
                },
            });
        }
    }
    if options.is_empty() {
        parent
            .spawn_empty()
            .apply_scene(label_dim(localizer.text("properties-events-no-targets")));
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
                    &localizer.text("properties-events-add"),
                    &localizer.text("properties-events-add-description"),
                    &options,
                    230.0,
                );
            });
    }
}

fn localized_event_trigger(localizer: &Localizer, trigger: EventTrigger) -> String {
    localizer.text(match trigger {
        EventTrigger::OnSpawn => "properties-event-on-spawn",
        EventTrigger::OnDeath => "properties-event-on-death",
        EventTrigger::OnCollision => "properties-event-on-collision",
    })
}

fn properties_module_collapsed(settings: &EditorSettings, module: &ModuleInstance) -> bool {
    properties_module_card_memory(module).collapsed(&settings.properties.section_expansion)
}

fn properties_renderer_collapsed(
    settings: &EditorSettings,
    renderer: &aestra_bevy::RendererInstance,
) -> bool {
    properties_renderer_card_memory(renderer).collapsed(&settings.properties.section_expansion)
}

fn properties_module_card_memory(module: &ModuleInstance) -> RememberedPanelCard {
    RememberedPanelCard::new(
        properties_module_key(module),
        !matches!(module.stage, StageKind::ParticleUpdate),
    )
}

fn properties_renderer_card_memory(
    renderer: &aestra_bevy::RendererInstance,
) -> RememberedPanelCard {
    RememberedPanelCard::new(properties_renderer_key(renderer), false)
}

fn properties_module_key(module: &ModuleInstance) -> String {
    format!("module/{}", module.module_type.0)
}

fn properties_renderer_key(renderer: &aestra_bevy::RendererInstance) -> String {
    match renderer.properties {
        RendererProperties::Sprite => "renderer/sprite",
        RendererProperties::Flipbook { .. } => "renderer/flipbook",
        _ => "renderer/unknown",
    }
    .into()
}

pub(crate) fn toggle_persisted_properties_section(
    session: &EditorSession,
    settings: &mut EditorSettings,
    section: PropertiesSection,
) -> bool {
    let card = match section {
        PropertiesSection::Module(id) => {
            let Some(module) = session
                .selected_layer()
                .modules
                .iter()
                .find(|module| module.id == id)
            else {
                return false;
            };
            properties_module_card_memory(module)
        }
        PropertiesSection::Renderer(id) => {
            let Some(renderer) = session
                .selected_layer()
                .renderers
                .iter()
                .find(|renderer| renderer.id == id)
            else {
                return false;
            };
            properties_renderer_card_memory(renderer)
        }
    };
    card.toggle(&mut settings.properties.section_expansion);
    true
}

fn spawn_start_reference_controls(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    target: StartReferenceTarget,
    localizer: &Localizer,
) {
    let reference = start_reference(session, target);
    let fallback_marker = reference
        .map(|reference| reference.marker)
        .or_else(|| session.effect.markers.first().map(|marker| marker.id));
    let mut mode_options = vec![ComboOption {
        label: localizer.text("properties-start-mode-absolute"),
        selected: reference.is_none(),
        action: PropertiesAction::SetStartReference {
            target,
            marker: None,
        },
    }];
    if let Some(marker) = fallback_marker {
        mode_options.push(ComboOption {
            label: localizer.text("properties-start-mode-marker"),
            selected: reference.is_some(),
            action: PropertiesAction::SetStartReference {
                target,
                marker: Some(marker),
            },
        });
    }
    let current_mode = if reference.is_some() {
        localizer.text("properties-start-mode-marker")
    } else {
        localizer.text("properties-start-mode-absolute")
    };
    spawn_properties_combo_row(
        parent,
        &localizer.text("properties-start-mode"),
        &current_mode,
        &mode_options,
        Some(&localizer.text("properties-start-mode-description")),
    );

    let Some(reference) = reference else {
        return;
    };
    let marker_options = session
        .effect
        .markers
        .iter()
        .map(|marker| ComboOption {
            label: marker.name.clone(),
            selected: marker.id == reference.marker,
            action: PropertiesAction::SetStartReference {
                target,
                marker: Some(marker.id),
            },
        })
        .collect::<Vec<_>>();
    let marker_name = session
        .effect
        .markers
        .iter()
        .find(|marker| marker.id == reference.marker)
        .map_or_else(
            || reference.marker.to_string(),
            |marker| marker.name.clone(),
        );
    spawn_properties_combo_row(
        parent,
        &localizer.text("properties-start-marker"),
        &marker_name,
        &marker_options,
        Some(&localizer.text("properties-start-marker-description")),
    );
    crate::feathers::field_row::spawn_field_row(
        parent,
        crate::feathers::field_row::FieldRowProps::new(localizer.text("properties-start-offset"))
            .with_control_min_width(150.0),
        EditorTooltip::description(localizer.text("properties-start-offset-description")),
        |controls| {
            controls
                .spawn_empty()
                .apply_scene(ui_shell::feathers_scalar_input())
                .insert((
                    StartReferenceOffsetControl { target },
                    AccessibleLabel(localizer.text("properties-start-offset")),
                ));
        },
    );
}

fn spawn_emitter_timing_controls(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    spawn_start_reference_controls(
        parent,
        session,
        StartReferenceTarget::Emitter(session.selected_layer().id),
        localizer,
    );
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
            mini_button(row, "+", PropertiesAction::OpenModulePalette(stage));
        });
}

fn spawn_module_card(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    metadata: Option<&ModuleMetadata>,
    diagnostic_path: &str,
    session: &EditorSession,
    localizer: &Localizer,
    collapsed: bool,
    asset_server: &AssetServer,
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
            .with_memory_key(properties_module_key(module))
            .with_help(help)
            .with_enabled(module.enabled)
            .with_background(if module.enabled {
                theme::PANEL_LIGHT
            } else {
                theme::PANEL_DARK
            })
            .with_border(base_border),
        PropertiesSemanticTarget {
            target: SemanticTarget::Module(module.id),
            base_border,
        },
        PropertiesSelectionTarget(SemanticTarget::Module(module.id)),
        PropertiesAction::ToggleSection(PropertiesSection::Module(module.id)),
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
                        action: PropertiesAction::MoveModule(module.id, -1),
                    },
                    ComboOption {
                        label: "Move down".into(),
                        selected: false,
                        action: PropertiesAction::MoveModule(module.id, 1),
                    },
                    ComboOption {
                        label: "Duplicate".into(),
                        selected: false,
                        action: PropertiesAction::DuplicateModule(module.id),
                    },
                    ComboOption {
                        label: "Delete…".into(),
                        selected: false,
                        action: PropertiesAction::DeleteModule(module.id),
                    },
                ],
            );
        },
        |card| {
            if let Some(metadata) = metadata {
                for (input_index, input) in metadata.inputs.iter().enumerate() {
                    spawn_input_control(
                        card,
                        module,
                        input,
                        input_index as u8,
                        session,
                        localizer,
                        asset_server,
                    );
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
    session: &EditorSession,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    let display_name = localized_properties_input(localizer, input.name, input.display_name, false);
    let description = localized_properties_input(localizer, input.name, input.description, true);
    let Some(value) = properties_module_parameter(session, module.id, input.name) else {
        spawn_properties_read_only_control(parent, &display_name, "Missing authored value");
        return;
    };
    let public =
        public_module_input_control(session, module, input, input_index, &value, localizer);
    let source = property_source_for_input(module, input, &value);
    if input.sources.len() > 1
        && matches!(&input.control, InputControl::Number { .. })
        && matches!(&value, Value::Scalar(_))
    {
        spawn_properties_scalar_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            public,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    if source == PropertySourceKind::RandomRange && matches!(&value, Value::Range(_)) {
        spawn_properties_range_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            public,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    if matches!(source, PropertySourceKind::Curve(_))
        && let Value::Curve(curve) = &value
    {
        spawn_properties_curve_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            curve,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    match (&input.control, value) {
        (InputControl::Curve { .. }, Value::Curve(curve)) => {
            spawn_properties_curve_source_control(
                parent,
                module.id,
                input,
                input_index,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                &curve,
                source,
                asset_server,
                localizer,
            );
        }
        (InputControl::Gradient, Value::Gradient(gradient)) => {
            spawn_properties_gradient_source_control(
                parent,
                module.id,
                input,
                input_index,
                &display_name,
                &description,
                &gradient,
                source,
                asset_server,
                localizer,
            );
        }
        (InputControl::Toggle, Value::Bool(value)) => {
            spawn_properties_toggle_control(
                parent,
                module.id,
                input,
                &display_name,
                &description,
                value,
                public,
            );
        }
        (InputControl::Number { .. }, Value::U32(_)) => {
            spawn_properties_integer_control(
                parent,
                module.id,
                input,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                public,
            );
        }
        (InputControl::Number { step, min, max }, Value::Scalar(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Scalar,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("", value, 0)],
                public,
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec2(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1)],
                public,
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec3(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1), ("Z", value[2], 2)],
                public,
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec4(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Vector,
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
                public,
            );
        }
        (InputControl::Range { .. }, Value::Range(_)) => {
            spawn_properties_range_source_control(
                parent,
                module.id,
                input,
                input_index,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                public,
                source,
                asset_server,
                localizer,
            );
        }
        (InputControl::Choice, value) => spawn_properties_choice_control(
            parent,
            module.id,
            input_index,
            &display_name,
            &description,
            &value,
        ),
        (_, value) => {
            spawn_properties_read_only_control(parent, &display_name, &format_value(value));
        }
    }
}

pub(crate) fn localized_properties_input(
    localizer: &Localizer,
    input: &str,
    fallback: &str,
    description: bool,
) -> String {
    let message = match (input, description) {
        ("spawn_rate", false) => "properties-input-spawn-rate",
        ("spawn_rate", true) => "properties-input-spawn-rate-description",
        ("burst_count", false) => "properties-input-burst-count",
        ("burst_count", true) => "properties-input-burst-count-description",
        ("shape", false) => "properties-input-shape",
        ("shape", true) => "properties-input-shape-description",
        ("lifetime", false) => "properties-input-lifetime",
        ("lifetime", true) => "properties-input-lifetime-description",
        ("speed", false) => "properties-input-speed",
        ("speed", true) => "properties-input-speed-description",
        ("direction", false) => "properties-input-direction",
        ("direction", true) => "properties-input-direction-description",
        ("spread_degrees", false) => "properties-input-spread",
        ("spread_degrees", true) => "properties-input-spread-description",
        ("angular_velocity", false) => "properties-input-angular-velocity",
        ("angular_velocity", true) => "properties-input-angular-velocity-description",
        ("gravity", false) => "properties-input-gravity",
        ("gravity", true) => "properties-input-gravity-description",
        ("drag", false) => "properties-input-drag",
        ("drag", true) => "properties-input-drag-description",
        ("turbulence", false) => "properties-input-turbulence",
        ("turbulence", true) => "properties-input-turbulence-description",
        ("size", false) => "properties-input-size",
        ("size", true) => "properties-input-size-description",
        ("opacity", false) => "properties-input-opacity",
        ("opacity", true) => "properties-input-opacity-description",
        ("color", false) => "properties-input-color",
        ("color", true) => "properties-input-color-description",
        _ => return fallback.to_owned(),
    };
    localizer.text(message)
}

fn property_tooltip(description: &str, unit: Option<&str>, localizer: &Localizer) -> EditorTooltip {
    let tooltip = EditorTooltip::description(description);
    let Some(unit) = unit else {
        return tooltip;
    };
    let mut args = FluentArgs::new();
    args.set("unit", unit);
    tooltip.with_footer(localizer.text_with("properties-property-unit", &args))
}

#[derive(Debug, Clone)]
struct ModuleInputPublicControl {
    module: ModuleId,
    input: u8,
    is_public: bool,
    label: String,
    description: String,
}

#[derive(Component)]
struct ModuleInputPublicRow;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct ModuleInputPublicToggle {
    is_public: bool,
}

fn public_module_input_control(
    session: &EditorSession,
    module: &ModuleInstance,
    input: &InputMetadata,
    input_index: u8,
    value: &Value,
    localizer: &Localizer,
) -> Option<ModuleInputPublicControl> {
    let supported = matches!(
        (&input.control, value),
        (InputControl::Toggle, Value::Bool(_))
            | (
                InputControl::Number { .. },
                Value::U32(_) | Value::Scalar(_) | Value::Range(_) | Value::Curve(_)
            )
            | (
                InputControl::Vector { .. },
                Value::Vec2(_) | Value::Vec3(_) | Value::Vec4(_)
            )
            | (InputControl::Range { .. }, Value::Range(_))
    );
    if !supported {
        return None;
    }
    let is_public = module
        .bindings
        .get(input.name)
        .and_then(|parameter| {
            session
                .effect
                .parameters
                .iter()
                .find(|candidate| candidate.id == *parameter)
        })
        .is_some_and(|parameter| parameter.exposed);
    Some(ModuleInputPublicControl {
        module: module.id,
        input: input_index,
        is_public,
        label: if is_public {
            localizer.text("properties-make-module-input-private")
        } else {
            localizer.text("properties-expose-module-input")
        },
        description: if is_public {
            localizer.text("properties-make-module-input-private-description")
        } else {
            localizer.text("properties-expose-module-input-description")
        },
    })
}

fn spawn_module_input_public_toggle(
    parent: &mut ChildSpawnerCommands,
    public: Option<ModuleInputPublicControl>,
) {
    let Some(public) = public else {
        return;
    };
    let button = mini_button(
        parent,
        "P",
        PropertiesAction::ToggleModuleInputPublic {
            module: public.module,
            input: public.input,
        },
    );
    parent.commands().entity(button).insert((
        ModuleInputPublicToggle {
            is_public: public.is_public,
        },
        AccessibleLabel(public.label),
        EditorTooltip::description(public.description),
        if public.is_public {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        },
    ));
    if public.is_public {
        parent
            .commands()
            .entity(button)
            .insert((Selected, ButtonVariant::Primary));
    }
}

fn sync_module_input_public_toggle_visibility(
    rows: Query<&RelativeCursorPosition, With<ModuleInputPublicRow>>,
    mut toggles: Query<(&ModuleInputPublicToggle, &ChildOf, &mut Visibility)>,
) {
    for (toggle, parent, mut visibility) in &mut toggles {
        let row_hovered = rows
            .get(parent.parent())
            .is_ok_and(RelativeCursorPosition::cursor_over);
        *visibility = if toggle.is_public || row_hovered {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn spawn_properties_range_source_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    input_index: u8,
    title: &str,
    tooltip: EditorTooltip,
    public: Option<ModuleInputPublicControl>,
    source: PropertySourceKind,
    asset_server: &AssetServer,
    localizer: &Localizer,
) {
    let Some((step, min, max)) = numeric_source_limits(&input.control) else {
        return;
    };
    parent
        .spawn((
            tooltip,
            ModuleInputPublicRow,
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|controls| {
                if source == PropertySourceKind::Constant {
                    controls
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((
                            PropertiesNumberControl {
                                module,
                                parameter: input.name,
                                component: 0,
                                kind: PropertiesNumberKind::RangeConstant,
                                step,
                                min,
                                max,
                            },
                            AccessibleLabel(title.to_owned()),
                        ));
                } else {
                    for (axis, component) in [("MIN", 0), ("MAX", 1)] {
                        controls
                            .spawn(Node {
                                flex_grow: 1.0,
                                flex_basis: Val::Px(0.0),
                                min_width: Val::Px(44.0),
                                ..default()
                            })
                            .with_children(|wrapper| {
                                wrapper
                                    .spawn_empty()
                                    .apply_scene(ui_shell::feathers_labeled_scalar_input(
                                        axis,
                                        tokens::TEXT_INPUT_BG,
                                    ))
                                    .insert((
                                        PropertiesNumberControl {
                                            module,
                                            parameter: input.name,
                                            component,
                                            kind: PropertiesNumberKind::Range,
                                            step,
                                            min,
                                            max,
                                        },
                                        AccessibleLabel(format!("{title} {axis}")),
                                    ));
                            });
                    }
                }
            });
            spawn_module_input_public_toggle(row, public);
            spawn_property_source_menu(
                row,
                module,
                input_index,
                source,
                &input.sources,
                asset_server,
                localizer,
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_properties_scalar_source_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    input_index: u8,
    title: &str,
    tooltip: EditorTooltip,
    public: Option<ModuleInputPublicControl>,
    source: PropertySourceKind,
    asset_server: &AssetServer,
    localizer: &Localizer,
) {
    let InputControl::Number { step, min, max } = input.control else {
        return;
    };
    parent
        .spawn((
            tooltip,
            ModuleInputPublicRow,
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            })
            .with_children(|control| {
                control
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_scalar_input())
                    .insert((
                        PropertiesNumberControl {
                            module,
                            parameter: input.name,
                            component: 0,
                            kind: PropertiesNumberKind::Scalar,
                            step,
                            min,
                            max,
                        },
                        AccessibleLabel(title.to_owned()),
                    ));
            });
            spawn_module_input_public_toggle(row, public);
            spawn_property_source_menu(
                row,
                module,
                input_index,
                source,
                &input.sources,
                asset_server,
                localizer,
            );
        });
}

fn spawn_properties_curve_source_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    input_index: u8,
    title: &str,
    tooltip: EditorTooltip,
    curve: &Curve,
    source: PropertySourceKind,
    asset_server: &AssetServer,
    localizer: &Localizer,
) {
    let Some((step, min, max)) = properties_curve_limits(input, curve) else {
        return;
    };
    parent
        .spawn((
            tooltip,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            })
            .with_children(|control| {
                if source == PropertySourceKind::Constant {
                    control
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((
                            PropertiesNumberControl {
                                module,
                                parameter: input.name,
                                component: 0,
                                kind: PropertiesNumberKind::CurveConstant,
                                step,
                                min: Some(min),
                                max: Some(max),
                            },
                            AccessibleLabel(title.to_owned()),
                        ));
                } else {
                    spawn_property_source_editor_button(
                        control,
                        &format!("{} keys  →", curve.keys.len()),
                        title,
                        CurvesAction::OpenInput(module, input_index),
                    );
                }
            });
            spawn_property_source_menu(
                row,
                module,
                input_index,
                source,
                &input.sources,
                asset_server,
                localizer,
            );
        });
}

fn spawn_properties_gradient_source_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    input_index: u8,
    title: &str,
    description: &str,
    gradient: &Gradient,
    source: PropertySourceKind,
    asset_server: &AssetServer,
    localizer: &Localizer,
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
            spawn_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            })
            .with_children(|control| {
                let summary = if source == PropertySourceKind::Constant {
                    localizer.text("properties-source-constant-color")
                } else {
                    format!("{} color keys  →", gradient.keys.len())
                };
                spawn_property_source_editor_button(
                    control,
                    &summary,
                    title,
                    CurvesAction::OpenInput(module, input_index),
                );
            });
            spawn_property_source_menu(
                row,
                module,
                input_index,
                source,
                &input.sources,
                asset_server,
                localizer,
            );
        });
}

fn spawn_property_source_editor_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    accessible_label: &str,
    action: A,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(accessible_label.to_owned()),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(28.0),
                min_width: Val::Px(0.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                ..default()
            },
        ))
        .with_child((Text::new(label), ThemedText, Pickable::IGNORE));
}

fn spawn_property_source_menu(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: u8,
    current: PropertySourceKind,
    supported: &[PropertySourceKind],
    asset_server: &AssetServer,
    localizer: &Localizer,
) {
    let options = supported
        .iter()
        .map(|source| ComboOption {
            label: property_source_label(*source, localizer),
            selected: *source == current,
            action: PropertiesAction::SetModuleInputSource {
                module,
                input,
                source: *source,
            },
        })
        .collect::<Vec<_>>();
    let current_label = property_source_label(current, localizer);
    let mut args = FluentArgs::new();
    args.set("source", current_label.clone());
    spawn_icon_action_menu(
        parent,
        asset_server,
        property_source_icon(current),
        &localizer.text_with("properties-source-accessible", &args),
        &localizer.text_with("properties-source-tooltip", &args),
        &options,
    );
}

fn property_source_label(source: PropertySourceKind, localizer: &Localizer) -> String {
    localizer.text(match source {
        PropertySourceKind::Constant => "properties-source-constant",
        PropertySourceKind::RandomRange => "properties-source-random",
        PropertySourceKind::Curve(InputEvaluationDomain::ParticleLife) => {
            "properties-source-curve-particle-life"
        }
        PropertySourceKind::Curve(InputEvaluationDomain::EmitterTime) => {
            "properties-source-curve-emitter-time"
        }
        PropertySourceKind::Gradient(InputEvaluationDomain::ParticleLife) => {
            "properties-source-gradient-particle-life"
        }
        PropertySourceKind::Gradient(InputEvaluationDomain::EmitterTime) => {
            "properties-source-gradient-emitter-time"
        }
    })
}

fn property_source_icon(source: PropertySourceKind) -> &'static str {
    match source {
        PropertySourceKind::Constant => "icons/source-constant.svg",
        PropertySourceKind::RandomRange => "icons/source-random.svg",
        PropertySourceKind::Curve(_) => "icons/source-curve.svg",
        PropertySourceKind::Gradient(_) => "icons/source-gradient.svg",
    }
}

fn property_source_for_input(
    module: &ModuleInstance,
    input: &InputMetadata,
    value: &Value,
) -> PropertySourceKind {
    let source = module
        .property_source(input.name)
        .unwrap_or_else(|| PropertySourceKind::infer_legacy(value));
    if input.sources.contains(&source) {
        source
    } else {
        PropertySourceKind::Constant
    }
}

fn spawn_properties_integer_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    title: &str,
    tooltip: EditorTooltip,
    public: Option<ModuleInputPublicControl>,
) {
    let InputControl::Number { step, min, max } = input.control else {
        return;
    };
    parent
        .spawn((
            tooltip,
            ModuleInputPublicRow,
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_property_label(row, title);
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
                        PropertiesNumberControl {
                            module,
                            parameter: input.name,
                            component: 0,
                            kind: PropertiesNumberKind::U32,
                            step,
                            min,
                            max,
                        },
                        AccessibleLabel(title.to_owned()),
                    ));
            });
            spawn_module_input_public_toggle(row, public);
        });
}

fn spawn_properties_number_controls(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    tooltip: EditorTooltip,
    control: PropertiesNumberControl,
    values: &[(&'static str, f32, u8)],
    public: Option<ModuleInputPublicControl>,
) {
    let bounded_slider = values
        .first()
        .filter(|(axis, _, component)| {
            values.len() == 1
                && axis.is_empty()
                && *component == 0
                && control.kind == PropertiesNumberKind::Scalar
        })
        .and_then(|(_, value, _)| {
            control
                .min
                .zip(control.max)
                .and_then(|(min, max)| SliderRowProps::new(*value, min, max, control.step))
        });
    parent
        .spawn((
            tooltip,
            ModuleInputPublicRow,
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_property_label(row, title);
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
                            PropertiesSliderControl(control),
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
                                    PropertiesNumberControl {
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
            spawn_module_input_public_toggle(row, public);
        });
}

fn spawn_property_label(parent: &mut ChildSpawnerCommands, title: &str) {
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

fn spawn_properties_toggle_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    title: &str,
    description: &str,
    value: bool,
    public: Option<ModuleInputPublicControl>,
) {
    parent
        .spawn((
            EditorTooltip::description(description),
            ModuleInputPublicRow,
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            let mut checkbox = row.spawn_empty();
            checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                PropertiesToggleControl {
                    module,
                    parameter: input.name,
                },
                AccessibleLabel(title.to_owned()),
            ));
            if value {
                checkbox.insert(Checked);
            }
            spawn_module_input_public_toggle(row, public);
        });
}

fn spawn_properties_choice_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: u8,
    title: &str,
    description: &str,
    value: &Value,
) {
    let Value::Shape(shape) = value else {
        spawn_properties_read_only_control(parent, title, &format_value(value.clone()));
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
        action: PropertiesAction::SetModuleChoice {
            module,
            input,
            choice: choice as u8,
        },
    })
    .collect::<Vec<_>>();
    spawn_properties_combo_row(parent, title, current, &options, Some(description));
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
            spawn_property_label(row, title);
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
                                PropertiesNumberControl {
                                    module,
                                    parameter: "shape",
                                    component: *component,
                                    kind: PropertiesNumberKind::Shape,
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

fn spawn_properties_combo_row(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    current: &str,
    options: &[ComboOption<PropertiesAction>],
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
        spawn_property_label(row, title);
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
            spawn_property_label(row, title);
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
            spawn_property_label(row, title);
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

fn spawn_properties_read_only_control(parent: &mut ChildSpawnerCommands, title: &str, value: &str) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn_empty().apply_scene(label_dim(value.to_owned()));
        });
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
            .with_memory_key(properties_renderer_key(renderer))
            .with_help("Controls how this emitter is drawn.")
            .with_enabled(renderer.enabled)
            .with_border(base_border),
        PropertiesSemanticTarget {
            target: SemanticTarget::Renderer(renderer.id),
            base_border,
        },
        PropertiesSelectionTarget(SemanticTarget::Renderer(renderer.id)),
        PropertiesAction::ToggleSection(PropertiesSection::Renderer(renderer.id)),
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
                        action: PropertiesAction::DuplicateRenderer(renderer.id),
                    },
                    ComboOption {
                        label: "Delete…".into(),
                        selected: false,
                        action: PropertiesAction::DeleteRenderer(renderer.id),
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
                spawn_properties_read_only_control(card, "Material", "Missing");
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
                    action: PropertiesAction::SetRendererMaterial(renderer.id, index),
                })
                .collect::<Vec<_>>();
            spawn_properties_combo_row(card, "Material", &material.name, &material_options, None);
            let blend_options = [BlendMode::Alpha, BlendMode::Additive, BlendMode::Multiply]
                .into_iter()
                .map(|blend| ComboOption {
                    label: format!("{blend:?}"),
                    selected: blend == material.blend,
                    action: PropertiesAction::SetRendererBlend(renderer.id, blend),
                })
                .collect::<Vec<_>>();
            spawn_properties_combo_row(
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
                MaterialInput::Parameter(parameter) => spawn_properties_read_only_control(
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
                        action: PropertiesAction::SetRendererTexture(renderer.id, None),
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
                                action: PropertiesAction::SetRendererTexture(
                                    renderer.id,
                                    Some(index),
                                ),
                            }),
                    );
                    spawn_properties_combo_row(
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
                            action: PropertiesAction::SetRendererFlipbook(renderer.id, index),
                        })
                        .collect::<Vec<_>>();
                    spawn_properties_combo_row(
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
                        action: PropertiesAction::SetFlipbookTimeSource(renderer.id, candidate),
                    })
                    .collect::<Vec<_>>();
                    spawn_properties_combo_row(
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
                        action: PropertiesAction::SetFlipbookPlayback(renderer.id, candidate),
                    })
                    .collect::<Vec<_>>();
                    spawn_properties_combo_row(
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
                    stack_button(header, "×", PropertiesAction::CloseModulePalette, 28.0);
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
                    PropertiesAction::AddSpriteRenderer,
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
                    PropertiesAction::AddFlipbookRenderer,
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
                    PropertiesAction::AddModule(index),
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
    action: PropertiesAction,
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
    module.active_parameter_value(name)
}

fn select_properties_header(
    click: On<Pointer<Click>>,
    selectable: Query<&PropertiesSelectionTarget>,
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
        set_properties_status(
            &mut session,
            &localizer,
            PropertiesStatus::Selected(target.to_string()),
        );
        session.ui_revision += 1;
    }
}

#[derive(Component)]
pub(crate) struct PropertiesTitle;

#[derive(Component)]
struct PropertiesSemanticTarget {
    target: SemanticTarget,
    base_border: Color,
}

#[derive(Component, Debug, Clone, Copy)]
struct PropertiesSelectionTarget(SemanticTarget);

#[derive(Resource, Default)]
pub(crate) struct PropertiesFocus {
    pub(crate) target: Option<SemanticTarget>,
    pub(crate) wait_frames: u8,
    pub(crate) highlight: Option<SemanticTarget>,
    pub(crate) highlight_remaining: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropertiesNumberKind {
    U32,
    Scalar,
    CurveConstant,
    Vector,
    Range,
    RangeConstant,
    Shape,
}

#[derive(Component, Debug, Clone, Copy)]
struct PropertiesNumberControl {
    module: ModuleId,
    parameter: &'static str,
    component: u8,
    kind: PropertiesNumberKind,
    step: f32,
    min: Option<f32>,
    max: Option<f32>,
}

#[derive(Component, Debug, Clone, Copy)]
struct PropertiesSliderControl(PropertiesNumberControl);

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

#[derive(Component, Debug, Clone)]
struct EffectClipParameterNumberControl {
    clip: EffectClipId,
    parameter: ParameterId,
    value: Value,
    component: u8,
}

#[derive(Debug, Clone, Copy)]
struct EffectClipParameterScrubControl {
    clip: EffectClipId,
    parameter: ParameterId,
    component: u8,
    values: [f32; 4],
    kind: EffectClipParameterScrubKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectClipParameterScrubKind {
    U32,
    Scalar,
    Vec2,
    Vec3,
    Vec4,
    Range,
}

#[derive(Component, Debug, Clone)]
struct EffectClipParameterTextControl {
    clip: EffectClipId,
    parameter: ParameterId,
    value: String,
}

#[derive(Component, Debug, Clone, Copy)]
struct EffectClipParameterToggleControl {
    clip: EffectClipId,
    parameter: ParameterId,
    value: bool,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct EffectClipParameterOverrideIndicator(ParameterId);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct EffectClipParameterDiagnostic(ParameterId);

#[derive(Component, Debug, Clone, Copy)]
enum DocumentTextControl {
    Effect,
    Emitter,
    Marker(MarkerId),
    ChoreographyEvent(ChoreographyEventId),
}

#[derive(Component, Debug, Clone, Copy)]
struct MarkerNumberControl(MarkerId);

#[derive(Component, Debug, Clone, Copy)]
enum ChoreographyEventNumberControl {
    Time(ChoreographyEventId),
    Intensity(ChoreographyEventId),
}

#[derive(Component, Debug, Clone, Copy)]
struct ChoreographyEventPayloadTextControl(ChoreographyEventId);

#[derive(Component, Debug, Clone, Copy)]
struct StartReferenceOffsetControl {
    target: StartReferenceTarget,
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
    Properties(PropertiesNumberControl),
    Emitter(EmitterNumberControl),
    EffectClip(EffectClipNumberControl),
    EffectClipParameter(EffectClipParameterScrubControl),
    Renderer(RendererNumberControl),
    StartReferenceOffset(StartReferenceOffsetControl),
    ChoreographyEvent(ChoreographyEventNumberControl),
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
struct PropertiesToggleControl {
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
pub(crate) enum PropertiesSection {
    Module(ModuleId),
    Renderer(RendererId),
}
