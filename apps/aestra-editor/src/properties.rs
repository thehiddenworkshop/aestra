//! Properties ownership: module-stack UI, semantic property editing, numeric scrubbing,
//! navigation focus, and contextual help.

use crate::feathers::icon::load_svg_icon;
use crate::feathers::panel_card::{
    PanelCardProps, RememberedPanelCard, spawn_panel_card as spawn_remembered_panel_card,
};
use crate::feathers::slider_row::{SliderNumberInputPair, SliderRowProps, spawn_slider_input_pair};
use crate::timeline::{EffectClipChildSelection, EffectClipPath, TimelineState};
use crate::*;
use aestra_authoring::{MaterialAuthoringDocument, MaterialToolCommand, MaterialToolPlanner};
use aestra_compiler::{
    InputControl, InputEvaluationDomain, InputMetadata, InputSourceKind, MaterialCompiler,
    MaterialControlDescriptor, MaterialControlKind, MaterialControlSource, MaterialStackEntry,
    MaterialStackInsertTarget, MaterialStackMoveTarget, MaterialStackPresetKind,
    MaterialStackPresetTarget, MaterialStackProjection, MaterialStackProperty, ModuleRegistry,
};
use aestra_core::material::{
    MaterialCullMode, MaterialDepthTest, MaterialParameterValue, MaterialRenderState,
    MaterialRenderStatePolicy, MaterialValue, MaterialValueType,
};
use aestra_core::{
    ChoreographyEventId, ChoreographyEventKind, ChoreographyEventPayload, ColorKey, Curve,
    CurveKey, EffectAsset, EffectClip, EffectClipId, EffectParameter, Gradient, MarkerId,
    MarkerTimeReference, MaterialExpressionId, MaterialId, MaterialParameterId, MaterialProgramId,
    ParameterId, ScalarRange, ValueType, Vec3Curve, Vec3Range,
};
use bevy::{
    feathers::controls::ButtonVariant,
    ui::{BackgroundGradient, ColorStop, InteractionDisabled, LinearGradient, Selected},
    ui_widgets::{Activate, SliderValue},
};
use bevy_resvg::prelude::{SvgColor, UiSvg};
use fluent_bundle::FluentArgs;

mod module_controls;
mod referenced_effect;
mod renderer_controls;

pub(crate) use module_controls::PropertySourceKind;
#[cfg(test)]
use module_controls::{
    expose_module_input, preview_module_deletion, properties_module_key, set_module_input_source,
    toggle_module_input_public,
};
use module_controls::{
    handle_module_action, numeric_source_limits, properties_curve_limits,
    properties_module_card_memory, properties_module_collapsed, spawn_module_card,
};
pub(crate) use referenced_effect::EffectClipRepairState;
#[cfg(test)]
use referenced_effect::{
    EffectClipParameterIssue, effect_clip_breadcrumbs, effect_clip_parameter_entries,
};
use referenced_effect::{
    effect_clip_repair_source, spawn_effect_clip_properties,
    spawn_referenced_effect_clip_properties, spawn_referenced_emitter_properties,
    spawn_source_navigation_row, sync_effect_clip_properties_timing,
    sync_effect_clip_repair_candidates, update_effect_clip_repair_query,
};
#[cfg(test)]
use renderer_controls::properties_renderer_key;
use renderer_controls::{
    RendererNumberControl, RendererSliderControl, handle_material_stack_property_scalar_change,
    handle_material_stack_property_toggle_change, handle_renderer_action,
    handle_renderer_enabled_change, handle_renderer_scalar_change, handle_renderer_toggle_change,
    handle_semantic_material_scalar_change, handle_semantic_material_toggle_change,
    normalize_renderer_uv_scrub_value, properties_renderer_card_memory,
    properties_renderer_collapsed, renderer_number_input_value, renderer_number_step,
    renderer_numeric_scrub_command, set_semantic_material_render_state,
    set_semantic_material_source, spawn_renderer_card, sync_material_stack_property_number_inputs,
    sync_renderer_number_inputs, sync_renderer_slider_inputs, sync_semantic_material_number_inputs,
};

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
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<MaterialStackInspectorState>()
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
            .add_observer(handle_semantic_material_scalar_change)
            .add_observer(handle_semantic_material_toggle_change)
            .add_observer(handle_material_stack_property_scalar_change)
            .add_observer(handle_material_stack_property_toggle_change)
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
                        sync_semantic_material_number_inputs,
                        sync_material_stack_property_number_inputs,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticMaterialSourceChoice {
    Constant,
    RandomRange,
    EffectParameter(ParameterId),
    EmitterParameter(ParameterId),
}

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MaterialStackInspectorState {
    pub(crate) selected: Option<(MaterialProgramId, MaterialExpressionId)>,
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
    SetSemanticMaterialTexture {
        instance: MaterialId,
        parameter: MaterialParameterId,
        asset: usize,
    },
    SetSemanticMaterialSource {
        instance: MaterialId,
        parameter: MaterialParameterId,
        source: SemanticMaterialSourceChoice,
    },
    SetSemanticMaterialRenderState {
        instance: MaterialId,
        render_state: MaterialRenderState,
    },
    MoveSemanticMaterialModifier {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
        target_index: usize,
    },
    InsertSemanticMaterialModifier {
        program: MaterialProgramId,
        kind: aestra_compiler::MaterialStackModifierKind,
        target_index: usize,
    },
    InsertSemanticMaterialPreset {
        program: MaterialProgramId,
        preset: MaterialStackPresetKind,
        target_index: usize,
    },
    RemoveSemanticMaterialModifier {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
    },
    SetSemanticMaterialModifierEnabled {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
        enabled: bool,
    },
    InspectSemanticMaterialModifier {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
    },
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
    mut catalog: Option<ResMut<ProjectEffectCatalog>>,
    mut repair: Option<ResMut<EffectClipRepairState>>,
    mut material_history: Option<ResMut<MaterialProgramEditHistory>>,
    mut history_ledger: Option<ResMut<EditorHistoryLedger>>,
    mut material_stack_inspector: Option<ResMut<MaterialStackInspectorState>>,
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
                if handle_module_action(
                    *action,
                    &mut session,
                    &registry.0,
                    &mut palette,
                    &mut workspace,
                    &mut layout,
                    &localizer,
                ) {
                    continue;
                }
                if handle_renderer_action(
                    *action,
                    &mut session,
                    &mut palette,
                    &mut workspace,
                    &mut layout,
                ) {
                    continue;
                }
                match *action {
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
                    PropertiesAction::SetSemanticMaterialTexture {
                        instance,
                        parameter,
                        asset,
                    } => {
                        let Some(asset) = session.effect.assets.get(asset).map(|asset| asset.id)
                        else {
                            continue;
                        };
                        let Some(catalog) = catalog.as_deref() else {
                            session.status = "Material program catalog is unavailable".into();
                            continue;
                        };
                        match catalog.material_programs_for_effect(&session.effect) {
                            Ok(programs) => {
                                session.set_material_instance_parameter(
                                    &programs,
                                    instance,
                                    parameter,
                                    Some(MaterialParameterValue::Constant(
                                        MaterialValue::Texture2D(asset),
                                    )),
                                );
                            }
                            Err(error) => {
                                session.status = format!("Material program unavailable: {error}");
                            }
                        }
                    }
                    PropertiesAction::SetSemanticMaterialSource {
                        instance,
                        parameter,
                        source,
                    } => {
                        let Some(catalog) = catalog.as_deref() else {
                            session.status = "Material program catalog is unavailable".into();
                            continue;
                        };
                        set_semantic_material_source(
                            &mut session,
                            catalog,
                            instance,
                            parameter,
                            source,
                        );
                    }
                    PropertiesAction::SetSemanticMaterialRenderState {
                        instance,
                        render_state,
                    } => {
                        let Some(catalog) = catalog.as_deref() else {
                            session.status = "Material program catalog is unavailable".into();
                            continue;
                        };
                        set_semantic_material_render_state(
                            &mut session,
                            catalog,
                            instance,
                            render_state,
                        );
                    }
                    PropertiesAction::MoveSemanticMaterialModifier {
                        program,
                        expression,
                        target_index,
                    } => {
                        apply_material_program_edit(
                            &mut session,
                            catalog.as_deref_mut(),
                            material_history.as_deref_mut(),
                            history_ledger.as_deref_mut(),
                            program,
                            "Moved material modifier",
                            |_, current| {
                                MaterialCompiler
                                    .plan_stack_move(current, expression, target_index)
                                    .map(|plan| plan.replacement)
                                    .map_err(|error| error.to_string())
                            },
                        );
                    }
                    PropertiesAction::InsertSemanticMaterialModifier {
                        program,
                        kind,
                        target_index,
                    } => {
                        apply_material_program_edit(
                            &mut session,
                            catalog.as_deref_mut(),
                            material_history.as_deref_mut(),
                            history_ledger.as_deref_mut(),
                            program,
                            "Added material modifier",
                            |_, current| {
                                MaterialCompiler
                                    .plan_stack_insert(current, kind, target_index)
                                    .map(|plan| plan.replacement)
                                    .map_err(|error| error.to_string())
                            },
                        );
                    }
                    PropertiesAction::InsertSemanticMaterialPreset {
                        program,
                        preset,
                        target_index,
                    } => {
                        apply_material_program_edit(
                            &mut session,
                            catalog.as_deref_mut(),
                            material_history.as_deref_mut(),
                            history_ledger.as_deref_mut(),
                            program,
                            "Applied material preset",
                            |document, _| {
                                let command = MaterialToolCommand::ApplyMaterialPreset {
                                    program,
                                    preset,
                                    target_index,
                                };
                                let plan = MaterialToolPlanner::plan(document, command)
                                    .map_err(|error| error.to_string())?;
                                plan.replacement_program(program).cloned().ok_or_else(|| {
                                    format!(
                                        "material tool plan omitted replacement program {program}"
                                    )
                                })
                            },
                        );
                    }
                    PropertiesAction::RemoveSemanticMaterialModifier {
                        program,
                        expression,
                    } => {
                        apply_material_program_edit(
                            &mut session,
                            catalog.as_deref_mut(),
                            material_history.as_deref_mut(),
                            history_ledger.as_deref_mut(),
                            program,
                            "Removed material modifier",
                            |_, current| {
                                MaterialCompiler
                                    .plan_stack_remove(current, expression)
                                    .map(|plan| plan.replacement)
                                    .map_err(|error| error.to_string())
                            },
                        );
                    }
                    PropertiesAction::SetSemanticMaterialModifierEnabled {
                        program,
                        expression,
                        enabled,
                    } => {
                        apply_material_program_edit(
                            &mut session,
                            catalog.as_deref_mut(),
                            material_history.as_deref_mut(),
                            history_ledger.as_deref_mut(),
                            program,
                            if enabled {
                                "Enabled material modifier"
                            } else {
                                "Disabled material modifier"
                            },
                            |_, current| {
                                MaterialCompiler
                                    .plan_stack_set_enabled(current, expression, enabled)
                                    .map(|plan| plan.replacement)
                                    .map_err(|error| error.to_string())
                            },
                        );
                    }
                    PropertiesAction::InspectSemanticMaterialModifier {
                        program,
                        expression,
                    } => {
                        let selected = Some((program, expression));
                        if material_stack_inspector
                            .as_deref()
                            .is_some_and(|inspector| inspector.selected == selected)
                        {
                            continue;
                        }
                        if let Some(inspector) = material_stack_inspector.as_deref_mut() {
                            inspector.selected = selected;
                            session.ui_revision += 1;
                        } else {
                            session.status = "Material stack inspector is unavailable".into();
                        }
                    }
                    PropertiesAction::OpenModulePalette(_)
                    | PropertiesAction::CloseModulePalette
                    | PropertiesAction::AddModule(_)
                    | PropertiesAction::SetModuleChoice { .. }
                    | PropertiesAction::MoveModule(_, _)
                    | PropertiesAction::DuplicateModule(_)
                    | PropertiesAction::DeleteModule(_)
                    | PropertiesAction::ToggleModuleInputPublic { .. }
                    | PropertiesAction::SetModuleInputSource { .. } => unreachable!(),
                    PropertiesAction::AddSpriteRenderer
                    | PropertiesAction::AddFlipbookRenderer
                    | PropertiesAction::SetRendererMaterial(_, _)
                    | PropertiesAction::SetRendererBlend(_, _)
                    | PropertiesAction::SetRendererTexture(_, _)
                    | PropertiesAction::SetRendererFlipbook(_, _)
                    | PropertiesAction::SetFlipbookTimeSource(_, _)
                    | PropertiesAction::SetFlipbookPlayback(_, _)
                    | PropertiesAction::DuplicateRenderer(_)
                    | PropertiesAction::DeleteRenderer(_) => unreachable!(),
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

fn apply_material_program_edit(
    session: &mut EditorSession,
    catalog: Option<&mut ProjectEffectCatalog>,
    material_history: Option<&mut MaterialProgramEditHistory>,
    history_ledger: Option<&mut EditorHistoryLedger>,
    program: MaterialProgramId,
    label: &str,
    plan: impl FnOnce(
        &MaterialAuthoringDocument,
        &aestra_core::material::MaterialProgram,
    ) -> Result<aestra_core::material::MaterialProgram, String>,
) {
    let Some(catalog) = catalog else {
        session.status = "Material program catalog is unavailable".into();
        return;
    };
    let Some(material_history) = material_history else {
        session.status = "Material edit history is unavailable".into();
        return;
    };
    let Some(history_ledger) = history_ledger else {
        session.status = "Editor history is unavailable".into();
        return;
    };
    let result = catalog
        .material_programs_for_effect(&session.effect)
        .and_then(|programs| {
            let current = programs
                .iter()
                .find(|candidate| candidate.id == program)
                .cloned()
                .ok_or_else(|| format!("Material program {program} is unavailable"))?;
            let document = MaterialAuthoringDocument::new(session.effect.clone(), programs);
            let replacement = plan(&document, &current)?;
            material_history.execute_replacement(
                &session.effect,
                catalog,
                label,
                current,
                replacement,
            )
        });
    match result {
        Ok(()) => {
            history_ledger.record_material_edit(session);
            session.status = label.into();
        }
        Err(error) => session.status = format!("Material edit failed: {error}"),
    }
    session.ui_revision += 1;
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
    use crate::test_support;
    use aestra_core::{EffectClipSeed, EffectMarker};

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
        let mut source = test_support::effect_with_timing_slack();
        source.parameters = vec![
            aestra_core::EffectParameter {
                id: exposed,
                name: "Intensity".into(),
                default: Value::Scalar(2.0),
                exposed: true,
            },
            aestra_core::EffectParameter {
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
        let mut session = test_support::session_with_timing_slack();
        let source = EffectAssetRef::new(aestra_core::EffectId::from_u128(0xe11ec7));
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
        let mut session = test_support::session_with_timing_slack();
        let source = EffectAssetRef::new(aestra_core::EffectId::from_u128(0xe11ec7));
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
            Some(&Value::Range(aestra_core::ScalarRange {
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
    fn drag_sources_preserve_constant_random_and_particle_curve_values() {
        let mut session = test_support::session_with_timing_slack();
        clear_effect_parameters_and_bindings(&mut session);
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.parameter_value("drag").is_some())
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
            .position(|input| input.name == "drag")
            .unwrap() as u8;
        let original = properties_module_parameter(&session, module, "drag").unwrap();

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::RandomRange,
            &test_localizer(),
        ));
        let random = properties_module_parameter(&session, module, "drag").unwrap();
        assert!(matches!(random, Value::Range(_)));

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::ParticleLife),
            &test_localizer(),
        ));
        let curve = properties_module_parameter(&session, module, "drag").unwrap();
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
            properties_module_parameter(&session, module, "drag"),
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
            properties_module_parameter(&session, module, "drag"),
            Some(random)
        );
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::ParticleLife),
            &test_localizer(),
        ));
        assert_eq!(
            properties_module_parameter(&session, module, "drag"),
            Some(curve)
        );
        assert!(session.effect.validation_report().is_valid());
    }

    #[test]
    fn turbulence_sources_preserve_constant_random_and_particle_curve_values() {
        let mut session = test_support::session_with_timing_slack();
        clear_effect_parameters_and_bindings(&mut session);
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.parameter_value("turbulence").is_some())
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
            .position(|input| input.name == "turbulence")
            .unwrap() as u8;
        let original = properties_module_parameter(&session, module, "turbulence").unwrap();

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::RandomRange,
            &test_localizer(),
        ));
        let random = properties_module_parameter(&session, module, "turbulence").unwrap();
        assert!(matches!(random, Value::Range(_)));

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::ParticleLife),
            &test_localizer(),
        ));
        let curve = properties_module_parameter(&session, module, "turbulence").unwrap();
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
            properties_module_parameter(&session, module, "turbulence"),
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
            properties_module_parameter(&session, module, "turbulence"),
            Some(random)
        );
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::ParticleLife),
            &test_localizer(),
        ));
        assert_eq!(
            properties_module_parameter(&session, module, "turbulence"),
            Some(curve)
        );
        assert!(session.effect.validation_report().is_valid());
    }

    #[test]
    fn curve_output_range_is_editable_without_changing_its_normalized_shape() {
        let mut session = test_support::session_with_timing_slack();
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
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::EmitterTime),
            &test_localizer(),
        ));
        let Value::Curve(initial_curve) =
            properties_module_parameter(&session, module, "spawn_rate").unwrap()
        else {
            unreachable!()
        };
        let initial_min = initial_curve.output_range().min;
        let control = PropertiesNumberControl {
            module,
            parameter: "spawn_rate",
            component: 1,
            kind: PropertiesNumberKind::CurveOutputRange,
            step: 1.0,
            min: Some(0.0),
            max: None,
        };

        assert!(apply_properties_number(
            &mut session,
            control,
            60.0,
            &test_localizer(),
        ));
        assert!(apply_properties_number(
            &mut session,
            PropertiesNumberControl {
                component: 0,
                ..control
            },
            10.0,
            &test_localizer(),
        ));

        let Value::Curve(curve) =
            properties_module_parameter(&session, module, "spawn_rate").unwrap()
        else {
            panic!("spawn rate should remain curve-typed");
        };
        assert_eq!(curve.output_range(), ScalarRange::new(10.0, 60.0));
        assert!(curve.output_range.is_some());
        assert!(
            curve
                .keys
                .iter()
                .all(|key| (0.0..=1.0).contains(&key.value))
        );
        assert_eq!(
            properties_number_input_value(&session, control),
            Some(NumberInputValue::F32(60.0))
        );
        assert!(session.effect.validation_report().is_valid());

        session.undo();
        let Value::Curve(curve) =
            properties_module_parameter(&session, module, "spawn_rate").unwrap()
        else {
            unreachable!()
        };
        assert_eq!(curve.output_range(), ScalarRange::new(initial_min, 60.0));
    }

    #[test]
    fn gravity_source_switching_preserves_per_axis_random_and_curve_values() {
        let mut session = test_support::session_with_timing_slack();
        clear_effect_parameters_and_bindings(&mut session);
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.parameter_value("gravity").is_some())
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
            .position(|input| input.name == "gravity")
            .unwrap() as u8;

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::RandomRange,
            &test_localizer(),
        ));
        let random_control = PropertiesNumberControl {
            module,
            parameter: "gravity",
            component: 4,
            kind: PropertiesNumberKind::Vec3Range,
            step: 5.0,
            min: None,
            max: None,
        };
        assert!(apply_properties_number(
            &mut session,
            random_control,
            7.0,
            &test_localizer(),
        ));
        let random = properties_module_parameter(&session, module, "gravity").unwrap();
        let Value::Vec3Range(range) = random else {
            panic!("gravity random source should be vector-typed");
        };
        assert_eq!(range.max[1], 7.0);

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::Curve(InputEvaluationDomain::ParticleLife),
            &test_localizer(),
        ));
        let curve_control = PropertiesNumberControl {
            module,
            parameter: "gravity",
            component: 5,
            kind: PropertiesNumberKind::Vec3CurveOutputRange,
            step: 5.0,
            min: None,
            max: None,
        };
        assert!(apply_properties_number(
            &mut session,
            curve_control,
            20.0,
            &test_localizer(),
        ));
        let Value::Vec3Curve(curves) =
            properties_module_parameter(&session, module, "gravity").unwrap()
        else {
            panic!("gravity curve source should be vector-typed");
        };
        assert_eq!(curves.curves[2].output_range().max, 20.0);

        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::RandomRange,
            &test_localizer(),
        ));
        let Value::Vec3Range(range) =
            properties_module_parameter(&session, module, "gravity").unwrap()
        else {
            unreachable!()
        };
        assert_eq!(range.max[1], 7.0);
        assert!(session.effect.validation_report().is_valid());
    }

    #[test]
    fn gradient_property_preview_preserves_authored_stop_positions() {
        let gradient = Gradient::new(vec![
            ColorKey::new(0.0, [1.0, 0.0, 0.0, 1.0]),
            ColorKey::new(0.35, [0.0, 1.0, 0.0, 1.0]),
            ColorKey::new(1.0, [0.0, 0.0, 1.0, 1.0]),
        ]);

        let preview = gradient_preview_background(&gradient);
        let bevy::ui::Gradient::Linear(preview) = &preview.0[0] else {
            panic!("gradient property preview should be linear");
        };
        assert_eq!(preview.angle, LinearGradient::TO_RIGHT);
        assert_eq!(preview.stops.len(), 3);
        assert_eq!(preview.stops[0].point, Val::Percent(0.0));
        assert_eq!(preview.stops[1].point, Val::Percent(35.0));
        assert_eq!(preview.stops[2].point, Val::Percent(100.0));
    }

    #[test]
    fn public_spawn_rate_tracks_the_active_source_without_losing_alternates() {
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut replacement = test_support::effect_with_timing_slack();
        replacement.id = aestra_core::EffectId::from_u128(0xc41d);
        replacement.name = "Replacement".into();
        replacement.duration = 4.0;
        replacement.playback_mode = EffectPlaybackMode::Once;
        replacement.effect_clips.clear();
        replacement
            .save_ron(temporary.path().join("replacement.aestra.ron"))
            .unwrap();

        let missing = EffectAssetRef::new(aestra_core::EffectId::from_u128(0xdead));
        let replacement_ref = EffectAssetRef::new(replacement.id);
        let mut owner = test_support::effect_with_timing_slack();
        owner.id = aestra_core::EffectId::from_u128(0xa11ce);
        owner.effect_clips.clear();
        let mut clip = EffectClip::new(missing, 0.75, 1.5);
        clip.source_offset = 0.5;
        clip.transform.translation = [2.0, -1.0, 3.5];
        clip.seed = EffectClipSeed::Fixed(77);
        let clip_id = clip.id;
        owner.effect_clips.push(clip.clone());

        let mut session = test_support::session_with_timing_slack();
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
        let mut owner = test_support::effect_with_timing_slack();
        owner.id = aestra_core::EffectId::from_u128(0xa11ce);
        owner.effect_clips.clear();
        let missing = EffectAssetRef::new(aestra_core::EffectId::from_u128(0xdead));
        let mut clip = EffectClip::new(missing, 0.0, 3.5);
        clip.source_offset = 0.75;
        owner.effect_clips.push(clip.clone());

        let mut short = test_support::effect_with_timing_slack();
        short.id = aestra_core::EffectId::from_u128(0x5107);
        short.name = "Short".into();
        short.playback_mode = EffectPlaybackMode::Once;
        short.effect_clips.clear();
        short
            .save_ron(temporary.path().join("short.aestra.ron"))
            .unwrap();

        let mut cycle = test_support::effect_with_timing_slack();
        cycle.id = aestra_core::EffectId::from_u128(0xc1c1e);
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
        let session = test_support::session_with_timing_slack();
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
        app.insert_resource(test_support::session_with_timing_slack())
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
        let session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
    fn emitter_duration_editor_can_grow_the_source_within_the_effect() {
        let mut session = test_support::session_with_timing_slack();
        let original = session.selected_layer().clone();
        let available = session.effect.duration - original.start_time;
        assert!(available > original.duration);
        let desired = (original.duration + 0.4).min(available);
        let target = NumericScrubTarget::Emitter(EmitterNumberControl::Duration);

        assert!(preview_numeric_scrub(&mut session, target, desired));
        assert_eq!(session.selected_layer(), &original);
        commit_numeric_scrub(&mut session, target, desired, &test_localizer());

        let grown = session.selected_layer();
        assert!((grown.duration - desired).abs() <= 0.000_1);
        assert!((grown.timeline_regions()[0].duration - desired).abs() <= 0.000_1);
        session.undo();
        assert_eq!(session.selected_layer(), &original);
    }

    #[test]
    fn marker_offset_scrub_preserves_binding_and_commits_one_undoable_edit() {
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let session = test_support::session_with_timing_slack();
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
        let session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
        let clip = aestra_core::EffectClip::new(aestra_core::EffectId::from_u128(0xC11D), 0.0, 1.0);
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
        let mut leaf = test_support::effect_with_timing_slack();
        leaf.id = aestra_core::EffectId::from_u128(0x1EAF);
        leaf.name = "Leaf".into();
        leaf.effect_clips.clear();
        leaf.save_ron(temporary.path().join("leaf.aestra.ron"))
            .unwrap();

        let mut child = test_support::effect_with_timing_slack();
        child.id = aestra_core::EffectId::from_u128(0xC111D);
        child.name = "Child".into();
        child.effect_clips.clear();
        let nested = EffectClip::new(EffectAssetRef::new(leaf.id), 0.0, 1.0);
        let nested_id = nested.id;
        child.effect_clips.push(nested);
        child
            .save_ron(temporary.path().join("child.aestra.ron"))
            .unwrap();

        let mut root = test_support::effect_with_timing_slack();
        root.id = aestra_core::EffectId::from_u128(0xA007);
        root.name = "Root".into();
        root.effect_clips.clear();
        let parent = EffectClip::new(EffectAssetRef::new(child.id), 0.0, 1.0);
        let path = EffectClipPath::root_path(parent.id).child(nested_id);
        root.effect_clips.push(parent);
        let mut session = test_support::session_with_timing_slack();
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
    let region = session.selected_emitter_region();
    match control {
        EmitterNumberControl::Start => region.start_time,
        EmitterNumberControl::Duration => region.duration,
        EmitterNumberControl::End => region.end_time(),
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
        EmitterNumberControl::Start
        | EmitterNumberControl::Duration
        | EmitterNumberControl::End => None,
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
        EmitterNumberControl::Start
        | EmitterNumberControl::Duration
        | EmitterNumberControl::End => return None,
    }
    Some(())
}

fn selected_emitter_region_timing_transaction(
    session: &EditorSession,
    start_time: f32,
    duration: f32,
) -> Option<EffectTransaction> {
    let emitter = session.selected_layer();
    let selected = session
        .selected_emitter_region
        .unwrap_or_else(|| emitter.implicit_region_id());
    session.emitter_region_timing_transaction(
        emitter.id,
        selected,
        start_time,
        session.selected_emitter_region().source_offset,
        duration,
        "Changed emitter region timing",
    )
}

fn normalized_emitter_region_timing(
    session: &EditorSession,
    control: EmitterNumberControl,
    value: f32,
) -> (f32, f32) {
    let region = session.selected_emitter_region();
    let minimum = 0.05;
    match control {
        EmitterNumberControl::Start => {
            let start = value.clamp(0.0, (session.effect.duration - minimum).max(0.0));
            let duration = region
                .duration
                .min(session.effect.duration - start)
                .max(minimum);
            (start, duration)
        }
        EmitterNumberControl::Duration => {
            let duration = value.clamp(
                minimum,
                (session.effect.duration - region.start_time).max(minimum),
            );
            (region.start_time, duration)
        }
        EmitterNumberControl::End => {
            let end = value.clamp(region.start_time + minimum, session.effect.duration);
            (region.start_time, end - region.start_time)
        }
        _ => (region.start_time, region.duration),
    }
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
            .map(|_| NumberInputValue::F32(curve.sample(0.0))),
        (PropertiesNumberKind::CurveOutputRange, Value::Curve(curve)) => {
            let range = curve.output_range();
            Some(NumberInputValue::F32(if control.component == 0 {
                range.min
            } else {
                range.max
            }))
        }
        (PropertiesNumberKind::Vec3CurveOutputRange, Value::Vec3Curve(curves)) => {
            let axis = control.component as usize / 2;
            let bound = control.component as usize % 2;
            let range = curves.curves.get(axis)?.output_range();
            Some(NumberInputValue::F32(if bound == 0 {
                range.min
            } else {
                range.max
            }))
        }
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
        (PropertiesNumberKind::Vec3Range, Value::Vec3Range(value)) => {
            let component = control.component as usize;
            let axis = component % 3;
            Some(NumberInputValue::F32(if component < 3 {
                value.min[axis]
            } else {
                value.max[axis]
            }))
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
        EmitterNumberControl::Start
        | EmitterNumberControl::Duration
        | EmitterNumberControl::End => {
            let (start_time, duration) =
                normalized_emitter_region_timing(&session, *control, change.value);
            if let Some(transaction) =
                selected_emitter_region_timing_transaction(&session, start_time, duration)
            {
                session.execute_transaction(transaction, true);
            }
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
        EffectClipParameterScrubKind::Range => Value::Range(aestra_core::ScalarRange {
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
            EmitterNumberControl::Start
            | EmitterNumberControl::Duration
            | EmitterNumberControl::End => 0.05,
        },
        NumericScrubTarget::EffectClip(control) => match control.control {
            EmitterNumberControl::Translation(_) => 0.1,
            EmitterNumberControl::Rotation(_) => 1.0,
            EmitterNumberControl::Scale(_) => 0.05,
            EmitterNumberControl::Start
            | EmitterNumberControl::Duration
            | EmitterNumberControl::End => 0.05,
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
        NumericScrubTarget::Emitter(
            control @ (EmitterNumberControl::Start
            | EmitterNumberControl::Duration
            | EmitterNumberControl::End),
        ) => {
            let (start_time, duration) = normalized_emitter_region_timing(session, control, value);
            match control {
                EmitterNumberControl::Start => start_time,
                EmitterNumberControl::Duration => duration,
                EmitterNumberControl::End => start_time + duration,
                _ => unreachable!(),
            }
        }
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

fn preview_numeric_scrub(
    session: &mut EditorSession,
    target: NumericScrubTarget,
    value: f32,
) -> bool {
    if let NumericScrubTarget::Emitter(
        control @ (EmitterNumberControl::Start
        | EmitterNumberControl::Duration
        | EmitterNumberControl::End),
    ) = target
    {
        let (start_time, duration) = normalized_emitter_region_timing(session, control, value);
        let Some(transaction) =
            selected_emitter_region_timing_transaction(session, start_time, duration)
        else {
            return false;
        };
        return session.preview_interaction(transaction);
    }
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
        NumericScrubTarget::Emitter(
            _control @ (EmitterNumberControl::Start
            | EmitterNumberControl::Duration
            | EmitterNumberControl::End),
        ) => None,
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
        NumericScrubTarget::Emitter(
            control @ (EmitterNumberControl::Start
            | EmitterNumberControl::Duration
            | EmitterNumberControl::End),
        ) => {
            let (start_time, duration) = normalized_emitter_region_timing(session, control, value);
            if let Some(transaction) =
                selected_emitter_region_timing_transaction(session, start_time, duration)
            {
                session.execute_transaction(transaction, true);
            }
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
            curve.output_range = None;
            Some(Value::Curve(curve))
        }
        (PropertiesNumberKind::CurveOutputRange, Value::Curve(mut curve)) => {
            curve.normalize_output();
            let mut range = curve.output_range();
            if control.component == 0 {
                range.min = value.min(range.max);
            } else {
                range.max = value.max(range.min);
            }
            curve.output_range = Some(range);
            Some(Value::Curve(curve))
        }
        (PropertiesNumberKind::Vec3CurveOutputRange, Value::Vec3Curve(mut curves)) => {
            let axis = control.component as usize / 2;
            let bound = control.component as usize % 2;
            let curve = curves.curves.get_mut(axis)?;
            curve.normalize_output();
            let mut range = curve.output_range();
            if bound == 0 {
                range.min = value.min(range.max);
            } else {
                range.max = value.max(range.min);
            }
            curve.output_range = Some(range);
            Some(Value::Vec3Curve(curves))
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
        (PropertiesNumberKind::Vec3Range, Value::Vec3Range(mut range)) => {
            let component = control.component as usize;
            let axis = component % 3;
            if component < 3 {
                range.min[axis] = value.min(range.max[axis]);
            } else {
                range.max[axis] = value.max(range.min[axis]);
            }
            Some(Value::Vec3Range(range))
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
    material_stack_inspector: &MaterialStackInspectorState,
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
                                    catalog,
                                    properties_renderer_collapsed(settings, renderer),
                                    material_stack_inspector,
                                    asset_server,
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
    event: &aestra_core::ChoreographyEvent,
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
    if session.selected_layer().regions.is_empty() {
        spawn_start_reference_controls(
            parent,
            session,
            StartReferenceTarget::Emitter(session.selected_layer().id),
            localizer,
        );
    }
    parent
        .spawn((
            EditorTooltip::description(
                "Timeline start, duration, and derived end for the selected emitter region.",
            ),
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
                ("End", EmitterNumberControl::End),
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
                Value::Vec2(_)
                    | Value::Vec3(_)
                    | Value::Vec4(_)
                    | Value::Vec3Range(_)
                    | Value::Vec3Curve(_)
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

#[allow(clippy::too_many_arguments)]
fn spawn_properties_vector_source_control(
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
    let InputControl::Vector { step, min, max } = input.control else {
        return;
    };
    parent
        .spawn((
            tooltip,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                ..default()
            },
        ))
        .with_children(|column| {
            column
                .spawn((
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
                    if source == PropertySourceKind::Constant {
                        row.spawn(Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            column_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|controls| {
                            for (axis, component, sigil) in [
                                ("X", 0, tokens::TEXT_INPUT_X_AXIS),
                                ("Y", 1, tokens::TEXT_INPUT_Y_AXIS),
                                ("Z", 2, tokens::TEXT_INPUT_Z_AXIS),
                            ] {
                                controls
                                    .spawn(Node {
                                        flex_grow: 1.0,
                                        flex_basis: Val::Px(0.0),
                                        min_width: Val::Px(38.0),
                                        ..default()
                                    })
                                    .with_children(|wrapper| {
                                        wrapper
                                            .spawn_empty()
                                            .apply_scene(ui_shell::feathers_labeled_scalar_input(
                                                axis, sigil,
                                            ))
                                            .insert((
                                                PropertiesNumberControl {
                                                    module,
                                                    parameter: input.name,
                                                    component,
                                                    kind: PropertiesNumberKind::Vector,
                                                    step,
                                                    min,
                                                    max,
                                                },
                                                AccessibleLabel(format!("{title} {axis}")),
                                            ));
                                    });
                            }
                        });
                    } else {
                        row.spawn(Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            ..default()
                        });
                        if matches!(source, PropertySourceKind::Curve(_)) {
                            spawn_curve_source_editor_button(
                                row,
                                module,
                                input_index,
                                title,
                                asset_server,
                            );
                        }
                    }
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

            if source != PropertySourceKind::Constant {
                for (axis_index, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                    column
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(27.0),
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn((
                                Text::new(axis),
                                ThemedText,
                                TextFont {
                                    font_size: FontSize::Px(10.0),
                                    ..default()
                                },
                                Node {
                                    width: Val::Percent(36.0),
                                    min_width: Val::Px(82.0),
                                    padding: UiRect::left(Val::Px(12.0)),
                                    ..default()
                                },
                            ));
                            for (bound, label) in ["MIN", "MAX"].into_iter().enumerate() {
                                let (component, kind) = if source == PropertySourceKind::RandomRange
                                {
                                    (axis_index + bound * 3, PropertiesNumberKind::Vec3Range)
                                } else {
                                    (
                                        axis_index * 2 + bound,
                                        PropertiesNumberKind::Vec3CurveOutputRange,
                                    )
                                };
                                row.spawn(Node {
                                    flex_grow: 1.0,
                                    flex_basis: Val::Px(0.0),
                                    min_width: Val::Px(44.0),
                                    ..default()
                                })
                                .with_children(|wrapper| {
                                    wrapper
                                        .spawn_empty()
                                        .apply_scene(ui_shell::feathers_labeled_scalar_input(
                                            label,
                                            tokens::TEXT_INPUT_BG,
                                        ))
                                        .insert((
                                            PropertiesNumberControl {
                                                module,
                                                parameter: input.name,
                                                component: component as u8,
                                                kind,
                                                step,
                                                min,
                                                max,
                                            },
                                            AccessibleLabel(format!("{title} {axis} {label}")),
                                        ));
                                });
                            }
                        });
                }
            }
        });
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
                                kind: PropertiesNumberKind::CurveConstant,
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
                                            kind: PropertiesNumberKind::CurveOutputRange,
                                            step,
                                            min,
                                            max,
                                        },
                                        AccessibleLabel(format!("{title} {axis}")),
                                    ));
                            });
                    }
                    spawn_curve_source_editor_button(
                        controls,
                        module,
                        input_index,
                        title,
                        asset_server,
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
                spawn_property_gradient_editor_button(
                    control,
                    title,
                    CurvesAction::OpenInput(module, input_index),
                    gradient,
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

fn gradient_preview_background(gradient: &Gradient) -> BackgroundGradient {
    let stops = gradient
        .keys
        .iter()
        .map(|key| {
            ColorStop::percent(
                Color::srgba(key.color[0], key.color[1], key.color[2], key.color[3]),
                key.time.clamp(0.0, 1.0) * 100.0,
            )
        })
        .collect();
    BackgroundGradient::from(LinearGradient::to_right(stops))
}

fn spawn_property_gradient_editor_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    accessible_label: &str,
    action: A,
    gradient: &Gradient,
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
                padding: UiRect::all(Val::Px(5.0)),
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            gradient_preview_background(gradient),
            BorderColor::all(theme::BORDER_BRIGHT),
            Pickable::IGNORE,
        ));
}

fn spawn_curve_source_editor_button(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: u8,
    title: &str,
    asset_server: &AssetServer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            CurvesAction::OpenInput(module, input),
            FeathersActionButton,
            AccessibleLabel(title.to_owned()),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(28.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Px(12.0),
                height: Val::Px(12.0),
                ..default()
            },
            UiSvg(load_svg_icon(asset_server, "icons/chevron-right.svg")),
            SvgColor(theme::TEXT),
            Pickable::IGNORE,
        ));
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
    CurveOutputRange,
    Vec3CurveOutputRange,
    Vector,
    Range,
    RangeConstant,
    Vec3Range,
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
    End,
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
