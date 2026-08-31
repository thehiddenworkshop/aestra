use crate::{EffectCommand, EffectDiff, EffectTransaction, LockState, SemanticTarget};
use aestra_core::{
    DiagnosticCode, EffectAsset, EffectClip, EffectClipId, Emitter, EmitterId, MaterialId,
    ModuleId, ModuleParameters, RendererId, ValidationReport, Value,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("{kind} '{id}' was not found")]
    NotFound { kind: &'static str, id: String },
    #[error("{target} is locked")]
    Locked { target: SemanticTarget },
    #[error("index {index} is outside {collection} with length {len}")]
    IndexOutOfBounds {
        collection: &'static str,
        index: usize,
        len: usize,
    },
    #[error("module parameter '{parameter}' does not exist on this module type")]
    UnknownParameter { parameter: String },
    #[error("module parameter '{parameter}' expected {expected}, got {actual}")]
    ParameterType {
        parameter: String,
        expected: &'static str,
        actual: &'static str,
    },
    #[error("module parameter '{parameter}' cannot be removed from a built-in module")]
    RequiredParameter { parameter: String },
    #[error("module parameter '{parameter}' is not bound")]
    MissingBinding { parameter: String },
    #[error("transaction validation failed: {0}")]
    Validation(#[from] ValidationReport),
    #[error("the document changed after this transaction was previewed")]
    StalePreview,
}

#[derive(Debug, Clone)]
pub struct TransactionOutcome {
    pub inverse: EffectTransaction,
    pub diff: EffectDiff,
}

#[derive(Debug, Clone)]
pub struct TransactionPreview {
    source: EffectAsset,
    candidate: EffectAsset,
    transaction: EffectTransaction,
    diff: EffectDiff,
}

impl TransactionPreview {
    pub fn candidate(&self) -> &EffectAsset {
        &self.candidate
    }

    pub fn transaction(&self) -> &EffectTransaction {
        &self.transaction
    }

    pub fn diff(&self) -> &EffectDiff {
        &self.diff
    }

    pub(crate) fn source_matches(&self, effect: &EffectAsset) -> bool {
        &self.source == effect
    }

    pub(crate) fn into_transaction(self) -> EffectTransaction {
        self.transaction
    }
}

#[derive(Debug, Default)]
pub struct CommandExecutor;

impl CommandExecutor {
    pub fn preview(
        effect: &EffectAsset,
        locks: &LockState,
        transaction: EffectTransaction,
    ) -> Result<TransactionPreview, CommandError> {
        let source = effect.clone();
        let mut candidate = source.clone();
        let outcome = Self::execute(&mut candidate, locks, &transaction)?;
        Ok(TransactionPreview {
            source,
            candidate,
            transaction,
            diff: outcome.diff,
        })
    }

    pub fn execute(
        effect: &mut EffectAsset,
        locks: &LockState,
        transaction: &EffectTransaction,
    ) -> Result<TransactionOutcome, CommandError> {
        let before = effect.clone();
        let mut working = before.clone();
        let mut inverse_commands = Vec::new();

        for command in &transaction.commands {
            if let Some(target) = locks.blocking_target(command, &working) {
                return Err(CommandError::Locked { target });
            }
            let mut command_inverse = apply_command(&mut working, command)?;
            command_inverse.extend(inverse_commands);
            inverse_commands = command_inverse;
        }

        let mut report = working.validation_report();
        report.diagnostics.retain(|diagnostic| {
            !matches!(
                diagnostic.code,
                DiagnosticCode::MissingModule | DiagnosticCode::MissingRenderer
            )
        });
        report.into_result()?;
        let diff = EffectDiff::between(&before, &working);
        *effect = working;
        Ok(TransactionOutcome {
            inverse: EffectTransaction::new(
                format!("Undo {}", transaction.label),
                inverse_commands,
            ),
            diff,
        })
    }
}

fn apply_command(
    effect: &mut EffectAsset,
    command: &EffectCommand,
) -> Result<Vec<EffectCommand>, CommandError> {
    let inverse = match command {
        EffectCommand::SetEffectName { name } => {
            let previous = std::mem::replace(&mut effect.name, name.clone());
            vec![EffectCommand::SetEffectName { name: previous }]
        }
        EffectCommand::SetEffectDuration { duration } => {
            let previous = std::mem::replace(&mut effect.duration, *duration);
            vec![EffectCommand::SetEffectDuration { duration: previous }]
        }
        EffectCommand::SetEffectLooping { looping } => {
            let previous = std::mem::replace(&mut effect.looping, *looping);
            vec![EffectCommand::SetEffectLooping { looping: previous }]
        }
        EffectCommand::SetChoreographyOrder { order } => {
            let previous = std::mem::replace(&mut effect.choreography_order, order.clone());
            vec![EffectCommand::SetChoreographyOrder { order: previous }]
        }
        EffectCommand::AddMarker { marker, index } => {
            checked_insert(
                &mut effect.markers,
                *index,
                marker.clone(),
                "effect markers",
            )?;
            vec![EffectCommand::RemoveMarker { id: marker.id }]
        }
        EffectCommand::RemoveMarker { id } => {
            let index = marker_index(effect, *id)?;
            let marker = effect.markers.remove(index);
            vec![EffectCommand::AddMarker { marker, index }]
        }
        EffectCommand::SetMarkerName { id, name } => {
            let marker = marker_mut(effect, *id)?;
            let previous = std::mem::replace(&mut marker.name, name.clone());
            vec![EffectCommand::SetMarkerName {
                id: *id,
                name: previous,
            }]
        }
        EffectCommand::SetMarkerTime { id, time } => {
            let marker = marker_mut(effect, *id)?;
            let previous = std::mem::replace(&mut marker.time, *time);
            for emitter in &mut effect.emitters {
                if let Some(reference) = emitter.start_reference
                    && reference.marker == *id
                {
                    let start_time = *time + reference.offset;
                    let delta = start_time - emitter.start_time;
                    emitter.start_time = start_time;
                    for region in &mut emitter.regions {
                        region.start_time += delta;
                    }
                }
            }
            for clip in &mut effect.effect_clips {
                if let Some(reference) = clip.start_reference
                    && reference.marker == *id
                {
                    clip.start_time = *time + reference.offset;
                }
            }
            for event in &mut effect.choreography_events {
                if let Some(reference) = event.time_reference
                    && reference.marker == *id
                {
                    event.time = *time + reference.offset;
                }
            }
            vec![EffectCommand::SetMarkerTime {
                id: *id,
                time: previous,
            }]
        }
        EffectCommand::AddChoreographyEvent { event, index } => {
            checked_insert(
                &mut effect.choreography_events,
                *index,
                event.clone(),
                "choreography events",
            )?;
            vec![EffectCommand::RemoveChoreographyEvent { id: event.id }]
        }
        EffectCommand::RemoveChoreographyEvent { id } => {
            let index = choreography_event_index(effect, *id)?;
            let event = effect.choreography_events.remove(index);
            vec![EffectCommand::AddChoreographyEvent { event, index }]
        }
        EffectCommand::SetChoreographyEventName { id, name } => {
            let event = choreography_event_mut(effect, *id)?;
            let previous = std::mem::replace(&mut event.name, name.clone());
            vec![EffectCommand::SetChoreographyEventName {
                id: *id,
                name: previous,
            }]
        }
        EffectCommand::SetChoreographyEventTime { id, time } => {
            let marker_time = effect
                .choreography_events
                .iter()
                .find(|event| event.id == *id)
                .and_then(|event| event.time_reference)
                .map(|reference| marker_time(effect, reference.marker))
                .transpose()?;
            let event = choreography_event_mut(effect, *id)?;
            let previous = event.time;
            event.time = *time;
            if let (Some(reference), Some(marker_time)) = (&mut event.time_reference, marker_time) {
                reference.offset = *time - marker_time;
            }
            vec![EffectCommand::SetChoreographyEventTime {
                id: *id,
                time: previous,
            }]
        }
        EffectCommand::SetChoreographyEventTimeReference { id, reference } => {
            let resolved = reference
                .map(|reference| {
                    marker_time(effect, reference.marker).map(|time| time + reference.offset)
                })
                .transpose()?;
            let event = choreography_event_mut(effect, *id)?;
            let previous_reference = event.time_reference;
            let previous_time = event.time;
            event.time_reference = *reference;
            if let Some(resolved) = resolved {
                event.time = resolved;
            }
            vec![
                EffectCommand::SetChoreographyEventTimeReference {
                    id: *id,
                    reference: previous_reference,
                },
                EffectCommand::SetChoreographyEventTime {
                    id: *id,
                    time: previous_time,
                },
            ]
        }
        EffectCommand::SetChoreographyEventPayload { id, payload } => {
            let event = choreography_event_mut(effect, *id)?;
            let previous = std::mem::replace(&mut event.payload, payload.clone());
            vec![EffectCommand::SetChoreographyEventPayload {
                id: *id,
                payload: previous,
            }]
        }
        EffectCommand::AddEffectClip { clip, index } => {
            checked_insert(
                &mut effect.effect_clips,
                *index,
                clip.clone(),
                "effect clips",
            )?;
            vec![EffectCommand::RemoveEffectClip { id: clip.id }]
        }
        EffectCommand::RemoveEffectClip { id } => {
            let index = effect_clip_index(effect, *id)?;
            let clip = effect.effect_clips.remove(index);
            vec![EffectCommand::AddEffectClip { clip, index }]
        }
        EffectCommand::MoveEffectClip { id, index } => {
            let old_index = effect_clip_index(effect, *id)?;
            checked_move(&mut effect.effect_clips, old_index, *index, "effect clips")?;
            vec![EffectCommand::MoveEffectClip {
                id: *id,
                index: old_index,
            }]
        }
        EffectCommand::SetEffectClipTiming {
            id,
            start_time,
            source_offset,
            duration,
        } => {
            let marker_time = effect
                .effect_clips
                .iter()
                .find(|clip| clip.id == *id)
                .and_then(|clip| clip.start_reference)
                .map(|reference| marker_time(effect, reference.marker))
                .transpose()?;
            let clip = effect_clip_mut(effect, *id)?;
            let previous = (clip.start_time, clip.source_offset, clip.duration);
            clip.start_time = *start_time;
            if let (Some(reference), Some(marker_time)) = (&mut clip.start_reference, marker_time) {
                reference.offset = *start_time - marker_time;
            }
            clip.source_offset = *source_offset;
            clip.duration = *duration;
            vec![EffectCommand::SetEffectClipTiming {
                id: *id,
                start_time: previous.0,
                source_offset: previous.1,
                duration: previous.2,
            }]
        }
        EffectCommand::SetEffectClipStartReference { id, reference } => {
            let resolved = reference
                .map(|reference| {
                    marker_time(effect, reference.marker).map(|time| time + reference.offset)
                })
                .transpose()?;
            let clip = effect_clip_mut(effect, *id)?;
            let previous_reference = clip.start_reference;
            let previous_timing = (clip.start_time, clip.source_offset, clip.duration);
            clip.start_reference = *reference;
            if let Some(resolved) = resolved {
                clip.start_time = resolved;
            }
            vec![
                EffectCommand::SetEffectClipStartReference {
                    id: *id,
                    reference: previous_reference,
                },
                EffectCommand::SetEffectClipTiming {
                    id: *id,
                    start_time: previous_timing.0,
                    source_offset: previous_timing.1,
                    duration: previous_timing.2,
                },
            ]
        }
        EffectCommand::SetEffectClipSeed { id, seed } => {
            let clip = effect_clip_mut(effect, *id)?;
            let previous = std::mem::replace(&mut clip.seed, *seed);
            vec![EffectCommand::SetEffectClipSeed {
                id: *id,
                seed: previous,
            }]
        }
        EffectCommand::SetEffectClipSource { id, source } => {
            let clip = effect_clip_mut(effect, *id)?;
            let previous = std::mem::replace(&mut clip.source, *source);
            vec![EffectCommand::SetEffectClipSource {
                id: *id,
                source: previous,
            }]
        }
        EffectCommand::SetEffectClipTransform { id, transform } => {
            let clip = effect_clip_mut(effect, *id)?;
            let previous = std::mem::replace(&mut clip.transform, *transform);
            vec![EffectCommand::SetEffectClipTransform {
                id: *id,
                transform: previous,
            }]
        }
        EffectCommand::SetEffectClipParameterOverride {
            id,
            parameter,
            value,
        } => {
            let clip = effect_clip_mut(effect, *id)?;
            match clip.parameter_overrides.insert(*parameter, value.clone()) {
                Some(previous) => vec![EffectCommand::SetEffectClipParameterOverride {
                    id: *id,
                    parameter: *parameter,
                    value: previous,
                }],
                None => vec![EffectCommand::RemoveEffectClipParameterOverride {
                    id: *id,
                    parameter: *parameter,
                }],
            }
        }
        EffectCommand::RemoveEffectClipParameterOverride { id, parameter } => {
            let clip = effect_clip_mut(effect, *id)?;
            let previous = clip.parameter_overrides.remove(parameter).ok_or_else(|| {
                CommandError::UnknownParameter {
                    parameter: parameter.to_string(),
                }
            })?;
            vec![EffectCommand::SetEffectClipParameterOverride {
                id: *id,
                parameter: *parameter,
                value: previous,
            }]
        }
        EffectCommand::AddAsset { asset, index } => {
            checked_insert(&mut effect.assets, *index, asset.clone(), "effect assets")?;
            vec![EffectCommand::RemoveAsset { id: asset.id }]
        }
        EffectCommand::RemoveAsset { id } => {
            let index = effect
                .assets
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| not_found("asset", id))?;
            let asset = effect.assets.remove(index);
            vec![EffectCommand::AddAsset { asset, index }]
        }
        EffectCommand::AddParameter { parameter, index } => {
            checked_insert(
                &mut effect.parameters,
                *index,
                parameter.clone(),
                "effect parameters",
            )?;
            vec![EffectCommand::RemoveParameter { id: parameter.id }]
        }
        EffectCommand::RemoveParameter { id } => {
            let index = effect
                .parameters
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| not_found("parameter", id))?;
            let parameter = effect.parameters.remove(index);
            vec![EffectCommand::AddParameter { parameter, index }]
        }
        EffectCommand::SetParameter { id, parameter } => {
            let index = effect
                .parameters
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| not_found("parameter", id))?;
            let mut replacement = parameter.clone();
            replacement.id = *id;
            let previous = std::mem::replace(&mut effect.parameters[index], replacement);
            vec![EffectCommand::SetParameter {
                id: *id,
                parameter: previous,
            }]
        }
        EffectCommand::AddMaterial { material, index } => {
            checked_insert(
                &mut effect.materials,
                *index,
                material.clone(),
                "effect materials",
            )?;
            vec![EffectCommand::RemoveMaterial { id: material.id }]
        }
        EffectCommand::RemoveMaterial { id } => {
            let index = material_index(effect, *id)?;
            let material = effect.materials.remove(index);
            vec![EffectCommand::AddMaterial { material, index }]
        }
        EffectCommand::SetMaterial { id, material } => {
            let index = material_index(effect, *id)?;
            let mut replacement = material.clone();
            replacement.id = *id;
            let previous = std::mem::replace(&mut effect.materials[index], replacement);
            vec![EffectCommand::SetMaterial {
                id: *id,
                material: previous,
            }]
        }
        EffectCommand::AddFlipbook { flipbook, index } => {
            checked_insert(
                &mut effect.flipbooks,
                *index,
                flipbook.clone(),
                "effect flipbooks",
            )?;
            vec![EffectCommand::RemoveFlipbook { id: flipbook.id }]
        }
        EffectCommand::RemoveFlipbook { id } => {
            let index = effect
                .flipbooks
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| not_found("flipbook", id))?;
            let flipbook = effect.flipbooks.remove(index);
            vec![EffectCommand::AddFlipbook { flipbook, index }]
        }
        EffectCommand::SetFlipbook { id, flipbook } => {
            let index = effect
                .flipbooks
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| not_found("flipbook", id))?;
            let mut replacement = flipbook.clone();
            replacement.id = *id;
            let previous = std::mem::replace(&mut effect.flipbooks[index], replacement);
            vec![EffectCommand::SetFlipbook {
                id: *id,
                flipbook: previous,
            }]
        }
        EffectCommand::AddEmitter { emitter, index } => {
            checked_insert(
                &mut effect.emitters,
                *index,
                emitter.clone(),
                "effect emitters",
            )?;
            vec![EffectCommand::RemoveEmitter { id: emitter.id }]
        }
        EffectCommand::RemoveEmitter { id } => {
            let index = emitter_index(effect, *id)?;
            let emitter = effect.emitters.remove(index);
            let mut removed_events = Vec::new();
            let mut event_index = 0;
            while event_index < effect.events.len() {
                if effect.events[event_index].source == *id
                    || effect.events[event_index].target == *id
                {
                    removed_events.push((event_index, effect.events.remove(event_index)));
                } else {
                    event_index += 1;
                }
            }
            let mut commands = vec![EffectCommand::AddEmitter { emitter, index }];
            commands.extend(
                removed_events
                    .into_iter()
                    .map(|(index, event)| EffectCommand::AddEvent { event, index }),
            );
            commands
        }
        EffectCommand::MoveEmitter { id, index } => {
            let old_index = emitter_index(effect, *id)?;
            checked_move(&mut effect.emitters, old_index, *index, "effect emitters")?;
            vec![EffectCommand::MoveEmitter {
                id: *id,
                index: old_index,
            }]
        }
        EffectCommand::SetEmitterName { id, name } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = std::mem::replace(&mut emitter.name, name.clone());
            vec![EffectCommand::SetEmitterName {
                id: *id,
                name: previous,
            }]
        }
        EffectCommand::SetEmitterEnabled { id, enabled } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = std::mem::replace(&mut emitter.enabled, *enabled);
            vec![EffectCommand::SetEmitterEnabled {
                id: *id,
                enabled: previous,
            }]
        }
        EffectCommand::SetEmitterDisplayColor { id, color } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = std::mem::replace(&mut emitter.display_color, *color);
            vec![EffectCommand::SetEmitterDisplayColor {
                id: *id,
                color: previous,
            }]
        }
        EffectCommand::SetEmitterTransform { id, transform } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = std::mem::replace(&mut emitter.transform, *transform);
            vec![EffectCommand::SetEmitterTransform {
                id: *id,
                transform: previous,
            }]
        }
        EffectCommand::SetEmitterTiming {
            id,
            start_time,
            duration,
        } => {
            let marker_time = effect
                .emitters
                .iter()
                .find(|emitter| emitter.id == *id)
                .and_then(|emitter| emitter.start_reference)
                .map(|reference| marker_time(effect, reference.marker))
                .transpose()?;
            let emitter = emitter_mut(effect, *id)?;
            let previous = (emitter.start_time, emitter.duration);
            emitter.start_time = *start_time;
            if let (Some(reference), Some(marker_time)) =
                (&mut emitter.start_reference, marker_time)
            {
                reference.offset = *start_time - marker_time;
            }
            emitter.duration = *duration;
            vec![EffectCommand::SetEmitterTiming {
                id: *id,
                start_time: previous.0,
                duration: previous.1,
            }]
        }
        EffectCommand::SetEmitterRegions { id, regions } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = std::mem::replace(&mut emitter.regions, regions.clone());
            vec![EffectCommand::SetEmitterRegions {
                id: *id,
                regions: previous,
            }]
        }
        EffectCommand::SetEmitterStartReference { id, reference } => {
            let resolved = reference
                .map(|reference| {
                    marker_time(effect, reference.marker).map(|time| time + reference.offset)
                })
                .transpose()?;
            let emitter = emitter_mut(effect, *id)?;
            let previous_reference = emitter.start_reference;
            let previous_timing = (emitter.start_time, emitter.duration);
            let previous_regions = emitter.regions.clone();
            emitter.start_reference = *reference;
            if let Some(resolved) = resolved {
                let delta = resolved - emitter.start_time;
                emitter.start_time = resolved;
                for region in &mut emitter.regions {
                    region.start_time += delta;
                }
            }
            vec![
                EffectCommand::SetEmitterStartReference {
                    id: *id,
                    reference: previous_reference,
                },
                EffectCommand::SetEmitterTiming {
                    id: *id,
                    start_time: previous_timing.0,
                    duration: previous_timing.1,
                },
                EffectCommand::SetEmitterRegions {
                    id: *id,
                    regions: previous_regions,
                },
            ]
        }
        EffectCommand::SetEmitterCapacity { id, max_particles } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = std::mem::replace(&mut emitter.max_particles, *max_particles);
            vec![EffectCommand::SetEmitterCapacity {
                id: *id,
                max_particles: previous,
            }]
        }
        EffectCommand::AddModule {
            emitter,
            module,
            index,
        } => {
            let target = emitter_mut(effect, *emitter)?;
            checked_insert(
                &mut target.modules,
                *index,
                module.clone(),
                "emitter modules",
            )?;
            vec![EffectCommand::RemoveModule {
                emitter: *emitter,
                module: module.id,
            }]
        }
        EffectCommand::RemoveModule { emitter, module } => {
            let target = emitter_mut(effect, *emitter)?;
            let index = module_index(target, *module)?;
            let removed = target.modules.remove(index);
            vec![EffectCommand::AddModule {
                emitter: *emitter,
                module: removed,
                index,
            }]
        }
        EffectCommand::MoveModule {
            emitter,
            module,
            index,
        } => {
            let target = emitter_mut(effect, *emitter)?;
            let old_index = module_index(target, *module)?;
            checked_move(&mut target.modules, old_index, *index, "emitter modules")?;
            vec![EffectCommand::MoveModule {
                emitter: *emitter,
                module: *module,
                index: old_index,
            }]
        }
        EffectCommand::SetModuleEnabled {
            emitter,
            module,
            enabled,
        } => {
            let module = module_mut(effect, *emitter, *module)?;
            let previous = std::mem::replace(&mut module.enabled, *enabled);
            vec![EffectCommand::SetModuleEnabled {
                emitter: *emitter,
                module: module.id,
                enabled: previous,
            }]
        }
        EffectCommand::SetModuleParameter {
            emitter,
            module,
            parameter,
            value,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let previous = set_module_parameter(module_instance, parameter, value.clone())?;
            match previous {
                Some(value) => vec![EffectCommand::SetModuleParameter {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                    value,
                }],
                None => vec![EffectCommand::RemoveModuleParameter {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                }],
            }
        }
        EffectCommand::SetModulePropertySource {
            emitter,
            module,
            parameter,
            source,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let value = module_instance
                .property_value_for_source(parameter, *source)
                .ok_or_else(|| CommandError::UnknownParameter {
                    parameter: parameter.clone(),
                })?;
            if !source.accepts(&value) {
                return Err(CommandError::ParameterType {
                    parameter: parameter.clone(),
                    expected: property_source_expected_type(*source),
                    actual: value_type(&value),
                });
            }
            let previous = module_instance
                .property_sources
                .insert(parameter.clone(), *source);
            match previous {
                Some(source) => vec![EffectCommand::SetModulePropertySource {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                    source,
                }],
                None => vec![EffectCommand::RemoveModulePropertySource {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                }],
            }
        }
        EffectCommand::SetModulePropertySourceValue {
            emitter,
            module,
            parameter,
            source,
            value,
        } => {
            if *source == aestra_core::PropertySource::Constant || !source.accepts(value) {
                return Err(CommandError::ParameterType {
                    parameter: parameter.clone(),
                    expected: property_source_expected_type(*source),
                    actual: value_type(value),
                });
            }
            let module_instance = module_mut(effect, *emitter, *module)?;
            if module_instance.parameter_value(parameter).is_none() {
                return Err(CommandError::UnknownParameter {
                    parameter: parameter.clone(),
                });
            }
            let values = module_instance
                .property_source_values
                .entry(parameter.clone())
                .or_default();
            let previous = values
                .iter_mut()
                .find(|candidate| candidate.source == *source)
                .map(|candidate| std::mem::replace(&mut candidate.value, value.clone()));
            match previous {
                Some(value) => vec![EffectCommand::SetModulePropertySourceValue {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                    source: *source,
                    value,
                }],
                None => {
                    values.push(aestra_core::PropertySourceValue::new(
                        *source,
                        value.clone(),
                    ));
                    vec![EffectCommand::RemoveModulePropertySourceValue {
                        emitter: *emitter,
                        module: *module,
                        parameter: parameter.clone(),
                        source: *source,
                    }]
                }
            }
        }
        EffectCommand::RemoveModulePropertySourceValue {
            emitter,
            module,
            parameter,
            source,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let values = module_instance
                .property_source_values
                .get_mut(parameter)
                .ok_or_else(|| CommandError::UnknownParameter {
                    parameter: parameter.clone(),
                })?;
            let index = values
                .iter()
                .position(|candidate| candidate.source == *source)
                .ok_or_else(|| CommandError::UnknownParameter {
                    parameter: parameter.clone(),
                })?;
            let value = values.remove(index).value;
            if values.is_empty() {
                module_instance.property_source_values.remove(parameter);
            }
            vec![EffectCommand::SetModulePropertySourceValue {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                source: *source,
                value,
            }]
        }
        EffectCommand::RemoveModulePropertySource {
            emitter,
            module,
            parameter,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let source = module_instance
                .property_sources
                .remove(parameter)
                .ok_or_else(|| CommandError::UnknownParameter {
                    parameter: parameter.clone(),
                })?;
            vec![EffectCommand::SetModulePropertySource {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                source,
            }]
        }
        EffectCommand::RemoveModuleParameter {
            emitter,
            module,
            parameter,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let ModuleParameters::Custom(values) = &mut module_instance.parameters else {
                return Err(CommandError::RequiredParameter {
                    parameter: parameter.clone(),
                });
            };
            let value = values
                .remove(parameter)
                .ok_or_else(|| CommandError::UnknownParameter {
                    parameter: parameter.clone(),
                })?;
            let source = module_instance.property_sources.remove(parameter);
            let source_values = module_instance
                .property_source_values
                .remove(parameter)
                .unwrap_or_default();
            let mut inverse = vec![EffectCommand::SetModuleParameter {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                value,
            }];
            if let Some(source) = source {
                inverse.push(EffectCommand::SetModulePropertySource {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                    source,
                });
            }
            inverse.extend(source_values.into_iter().map(|source_value| {
                EffectCommand::SetModulePropertySourceValue {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                    source: source_value.source,
                    value: source_value.value,
                }
            }));
            inverse
        }
        EffectCommand::BindModuleParameter {
            emitter,
            module,
            parameter,
            source,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let previous = module_instance.bindings.insert(parameter.clone(), *source);
            match previous {
                Some(source) => vec![EffectCommand::BindModuleParameter {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                    source,
                }],
                None => vec![EffectCommand::UnbindModuleParameter {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                }],
            }
        }
        EffectCommand::UnbindModuleParameter {
            emitter,
            module,
            parameter,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let source = module_instance.bindings.remove(parameter).ok_or_else(|| {
                CommandError::MissingBinding {
                    parameter: parameter.clone(),
                }
            })?;
            vec![EffectCommand::BindModuleParameter {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                source,
            }]
        }
        EffectCommand::AddCurveKey {
            emitter,
            module,
            parameter,
            key,
            index,
        } => {
            let curve = module_curve_mut(effect, *emitter, *module, parameter)?;
            checked_insert(&mut curve.keys, *index, *key, "curve keys")?;
            vec![EffectCommand::RemoveCurveKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                index: *index,
            }]
        }
        EffectCommand::RemoveCurveKey {
            emitter,
            module,
            parameter,
            index,
        } => {
            let curve = module_curve_mut(effect, *emitter, *module, parameter)?;
            let key = checked_remove(&mut curve.keys, *index, "curve keys")?;
            vec![EffectCommand::AddCurveKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                key,
                index: *index,
            }]
        }
        EffectCommand::SetCurveKey {
            emitter,
            module,
            parameter,
            index,
            key,
        } => {
            let curve = module_curve_mut(effect, *emitter, *module, parameter)?;
            let previous = checked_replace(&mut curve.keys, *index, *key, "curve keys")?;
            vec![EffectCommand::SetCurveKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                index: *index,
                key: previous,
            }]
        }
        EffectCommand::AddGradientKey {
            emitter,
            module,
            parameter,
            key,
            index,
        } => {
            let gradient = module_gradient_mut(effect, *emitter, *module, parameter)?;
            checked_insert(&mut gradient.keys, *index, *key, "gradient keys")?;
            vec![EffectCommand::RemoveGradientKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                index: *index,
            }]
        }
        EffectCommand::RemoveGradientKey {
            emitter,
            module,
            parameter,
            index,
        } => {
            let gradient = module_gradient_mut(effect, *emitter, *module, parameter)?;
            let key = checked_remove(&mut gradient.keys, *index, "gradient keys")?;
            vec![EffectCommand::AddGradientKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                key,
                index: *index,
            }]
        }
        EffectCommand::SetGradientKey {
            emitter,
            module,
            parameter,
            index,
            key,
        } => {
            let gradient = module_gradient_mut(effect, *emitter, *module, parameter)?;
            let previous = checked_replace(&mut gradient.keys, *index, *key, "gradient keys")?;
            vec![EffectCommand::SetGradientKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                index: *index,
                key: previous,
            }]
        }
        EffectCommand::AddRenderer {
            emitter,
            renderer,
            index,
        } => {
            let target = emitter_mut(effect, *emitter)?;
            checked_insert(
                &mut target.renderers,
                *index,
                renderer.clone(),
                "emitter renderers",
            )?;
            vec![EffectCommand::RemoveRenderer {
                emitter: *emitter,
                renderer: renderer.id,
            }]
        }
        EffectCommand::RemoveRenderer { emitter, renderer } => {
            let target = emitter_mut(effect, *emitter)?;
            let index = renderer_index(target, *renderer)?;
            let removed = target.renderers.remove(index);
            vec![EffectCommand::AddRenderer {
                emitter: *emitter,
                renderer: removed,
                index,
            }]
        }
        EffectCommand::MoveRenderer {
            emitter,
            renderer,
            index,
        } => {
            let target = emitter_mut(effect, *emitter)?;
            let old_index = renderer_index(target, *renderer)?;
            checked_move(
                &mut target.renderers,
                old_index,
                *index,
                "emitter renderers",
            )?;
            vec![EffectCommand::MoveRenderer {
                emitter: *emitter,
                renderer: *renderer,
                index: old_index,
            }]
        }
        EffectCommand::SetRendererEnabled {
            emitter,
            renderer,
            enabled,
        } => {
            let renderer = renderer_mut(effect, *emitter, *renderer)?;
            let previous = std::mem::replace(&mut renderer.enabled, *enabled);
            vec![EffectCommand::SetRendererEnabled {
                emitter: *emitter,
                renderer: renderer.id,
                enabled: previous,
            }]
        }
        EffectCommand::SetRendererMaterial {
            emitter,
            renderer,
            material,
        } => {
            let renderer = renderer_mut(effect, *emitter, *renderer)?;
            let previous = std::mem::replace(&mut renderer.material, *material);
            vec![EffectCommand::SetRendererMaterial {
                emitter: *emitter,
                renderer: renderer.id,
                material: previous,
            }]
        }
        EffectCommand::SetRendererProperties {
            emitter,
            renderer,
            properties,
        } => {
            let renderer = renderer_mut(effect, *emitter, *renderer)?;
            let previous = std::mem::replace(&mut renderer.properties, properties.clone());
            vec![EffectCommand::SetRendererProperties {
                emitter: *emitter,
                renderer: renderer.id,
                properties: previous,
            }]
        }
        EffectCommand::AddEvent { event, index } => {
            checked_insert(&mut effect.events, *index, event.clone(), "effect events")?;
            vec![EffectCommand::RemoveEvent { id: event.id }]
        }
        EffectCommand::RemoveEvent { id } => {
            let index = effect
                .events
                .iter()
                .position(|event| event.id == *id)
                .ok_or_else(|| not_found("event", id))?;
            let event = effect.events.remove(index);
            vec![EffectCommand::AddEvent { event, index }]
        }
    };
    Ok(inverse)
}

fn set_module_parameter(
    module: &mut aestra_core::ModuleInstance,
    parameter: &str,
    value: Value,
) -> Result<Option<Value>, CommandError> {
    let inferred_source = aestra_core::PropertySource::infer_legacy(&value);
    let custom_parameter = matches!(module.parameters, ModuleParameters::Custom(_));
    let unknown = || CommandError::UnknownParameter {
        parameter: parameter.into(),
    };
    let mismatch = |expected, actual| CommandError::ParameterType {
        parameter: parameter.into(),
        expected,
        actual,
    };
    let previous = match (&mut module.parameters, value) {
        (ModuleParameters::Emission { spawn_rate, .. }, Value::Scalar(value))
            if parameter == "spawn_rate" =>
        {
            Some(Value::Scalar(std::mem::replace(spawn_rate, value)))
        }
        (ModuleParameters::Emission { burst_count, .. }, Value::U32(value))
            if parameter == "burst_count" =>
        {
            Some(Value::U32(std::mem::replace(burst_count, value)))
        }
        (ModuleParameters::Shape { shape }, Value::Shape(value)) if parameter == "shape" => {
            Some(Value::Shape(std::mem::replace(shape, value)))
        }
        (ModuleParameters::Initialize { lifetime, .. }, Value::Range(value))
            if parameter == "lifetime" =>
        {
            Some(Value::Range(std::mem::replace(lifetime, value)))
        }
        (ModuleParameters::Initialize { speed, .. }, Value::Range(value))
            if parameter == "speed" =>
        {
            Some(Value::Range(std::mem::replace(speed, value)))
        }
        (ModuleParameters::Initialize { direction, .. }, Value::Vec3(value))
            if parameter == "direction" =>
        {
            Some(Value::Vec3(std::mem::replace(direction, value)))
        }
        (ModuleParameters::Initialize { spread_degrees, .. }, Value::Scalar(value))
            if parameter == "spread_degrees" =>
        {
            Some(Value::Scalar(std::mem::replace(spread_degrees, value)))
        }
        (
            ModuleParameters::Initialize {
                angular_velocity, ..
            },
            Value::Range(value),
        ) if parameter == "angular_velocity" => {
            Some(Value::Range(std::mem::replace(angular_velocity, value)))
        }
        (ModuleParameters::Motion { gravity, .. }, Value::Vec3(value))
            if parameter == "gravity" =>
        {
            Some(Value::Vec3(std::mem::replace(gravity, value)))
        }
        (ModuleParameters::Motion { drag, .. }, Value::Scalar(value)) if parameter == "drag" => {
            Some(Value::Scalar(std::mem::replace(drag, value)))
        }
        (ModuleParameters::Motion { turbulence, .. }, Value::Scalar(value))
            if parameter == "turbulence" =>
        {
            Some(Value::Scalar(std::mem::replace(turbulence, value)))
        }
        (ModuleParameters::Appearance { size, .. }, Value::Curve(value)) if parameter == "size" => {
            Some(Value::Curve(std::mem::replace(size, value)))
        }
        (ModuleParameters::Appearance { opacity, .. }, Value::Curve(value))
            if parameter == "opacity" =>
        {
            Some(Value::Curve(std::mem::replace(opacity, value)))
        }
        (ModuleParameters::Appearance { color, .. }, Value::Gradient(value))
            if parameter == "color" =>
        {
            Some(Value::Gradient(std::mem::replace(color, value)))
        }
        (ModuleParameters::Custom(values), value) => values.insert(parameter.into(), value),
        (parameters, value) => {
            let expected = expected_parameter_type(parameters, parameter).ok_or_else(unknown)?;
            return Err(mismatch(expected, value_type(&value)));
        }
    };
    if custom_parameter && previous.is_none() {
        module
            .property_sources
            .entry(parameter.into())
            .or_insert(inferred_source);
    }
    Ok(previous)
}

fn expected_parameter_type(parameters: &ModuleParameters, parameter: &str) -> Option<&'static str> {
    match (parameters, parameter) {
        (ModuleParameters::Emission { .. }, "spawn_rate") => Some("scalar"),
        (ModuleParameters::Emission { .. }, "burst_count") => Some("u32"),
        (ModuleParameters::Shape { .. }, "shape") => Some("shape"),
        (ModuleParameters::Initialize { .. }, "lifetime" | "speed" | "angular_velocity") => {
            Some("range")
        }
        (ModuleParameters::Initialize { .. }, "direction") => Some("vec3"),
        (ModuleParameters::Initialize { .. }, "spread_degrees") => Some("scalar"),
        (ModuleParameters::Motion { .. }, "gravity") => Some("vec3"),
        (ModuleParameters::Motion { .. }, "drag" | "turbulence") => Some("scalar"),
        (ModuleParameters::Appearance { .. }, "size" | "opacity") => Some("curve"),
        (ModuleParameters::Appearance { .. }, "color") => Some("gradient"),
        _ => None,
    }
}

fn property_source_expected_type(source: aestra_core::PropertySource) -> &'static str {
    match source {
        aestra_core::PropertySource::Constant => "any value",
        aestra_core::PropertySource::RandomRange => "range",
        aestra_core::PropertySource::Curve(_) => "curve",
        aestra_core::PropertySource::Gradient(_) => "gradient",
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "bool",
        Value::U32(_) => "u32",
        Value::Scalar(_) => "scalar",
        Value::Vec2(_) => "vec2",
        Value::Vec3(_) => "vec3",
        Value::Vec3Range(_) => "vec3 range",
        Value::Vec3Curve(_) => "vec3 curve",
        Value::Vec4(_) => "vec4",
        Value::Text(_) => "text",
        Value::Range(_) => "range",
        Value::Curve(_) => "curve",
        Value::Gradient(_) => "gradient",
        Value::Shape(_) => "shape",
        Value::Parameter(_) => "parameter",
        Value::Asset(_) => "asset",
        Value::Material(_) => "material",
    }
}

fn emitter_index(effect: &EffectAsset, id: EmitterId) -> Result<usize, CommandError> {
    effect
        .emitters
        .iter()
        .position(|emitter| emitter.id == id)
        .ok_or_else(|| not_found("emitter", &id))
}

fn effect_clip_index(effect: &EffectAsset, id: EffectClipId) -> Result<usize, CommandError> {
    effect
        .effect_clips
        .iter()
        .position(|clip| clip.id == id)
        .ok_or_else(|| not_found("effect clip", &id))
}

fn effect_clip_mut(
    effect: &mut EffectAsset,
    id: EffectClipId,
) -> Result<&mut EffectClip, CommandError> {
    effect
        .effect_clips
        .iter_mut()
        .find(|clip| clip.id == id)
        .ok_or_else(|| not_found("effect clip", &id))
}

fn marker_index(effect: &EffectAsset, id: aestra_core::MarkerId) -> Result<usize, CommandError> {
    effect
        .markers
        .iter()
        .position(|marker| marker.id == id)
        .ok_or_else(|| not_found("marker", &id))
}

fn choreography_event_index(
    effect: &EffectAsset,
    id: aestra_core::ChoreographyEventId,
) -> Result<usize, CommandError> {
    effect
        .choreography_events
        .iter()
        .position(|event| event.id == id)
        .ok_or_else(|| not_found("choreography event", &id))
}

fn choreography_event_mut(
    effect: &mut EffectAsset,
    id: aestra_core::ChoreographyEventId,
) -> Result<&mut aestra_core::ChoreographyEvent, CommandError> {
    effect
        .choreography_events
        .iter_mut()
        .find(|event| event.id == id)
        .ok_or_else(|| not_found("choreography event", &id))
}

fn marker_time(effect: &EffectAsset, id: aestra_core::MarkerId) -> Result<f32, CommandError> {
    effect
        .markers
        .iter()
        .find(|marker| marker.id == id)
        .map(|marker| marker.time)
        .ok_or_else(|| not_found("marker", &id))
}

fn marker_mut(
    effect: &mut EffectAsset,
    id: aestra_core::MarkerId,
) -> Result<&mut aestra_core::EffectMarker, CommandError> {
    effect
        .markers
        .iter_mut()
        .find(|marker| marker.id == id)
        .ok_or_else(|| not_found("marker", &id))
}

fn material_index(effect: &EffectAsset, id: MaterialId) -> Result<usize, CommandError> {
    effect
        .materials
        .iter()
        .position(|material| material.id == id)
        .ok_or_else(|| not_found("material", &id))
}

fn emitter_mut(effect: &mut EffectAsset, id: EmitterId) -> Result<&mut Emitter, CommandError> {
    effect
        .emitters
        .iter_mut()
        .find(|emitter| emitter.id == id)
        .ok_or_else(|| not_found("emitter", &id))
}

fn module_index(emitter: &Emitter, id: ModuleId) -> Result<usize, CommandError> {
    emitter
        .modules
        .iter()
        .position(|module| module.id == id)
        .ok_or_else(|| not_found("module", &id))
}

fn renderer_index(emitter: &Emitter, id: RendererId) -> Result<usize, CommandError> {
    emitter
        .renderers
        .iter()
        .position(|renderer| renderer.id == id)
        .ok_or_else(|| not_found("renderer", &id))
}

fn module_mut(
    effect: &mut EffectAsset,
    emitter: EmitterId,
    module: ModuleId,
) -> Result<&mut aestra_core::ModuleInstance, CommandError> {
    emitter_mut(effect, emitter)?
        .modules
        .iter_mut()
        .find(|item| item.id == module)
        .ok_or_else(|| not_found("module", &module))
}

fn renderer_mut(
    effect: &mut EffectAsset,
    emitter: EmitterId,
    renderer: RendererId,
) -> Result<&mut aestra_core::RendererInstance, CommandError> {
    emitter_mut(effect, emitter)?
        .renderers
        .iter_mut()
        .find(|item| item.id == renderer)
        .ok_or_else(|| not_found("renderer", &renderer))
}

fn module_curve_mut<'a>(
    effect: &'a mut EffectAsset,
    emitter: EmitterId,
    module: ModuleId,
    parameter: &str,
) -> Result<&'a mut aestra_core::Curve, CommandError> {
    let module = module_mut(effect, emitter, module)?;
    let payload_index = module.property_source(parameter).and_then(|source| {
        matches!(source, aestra_core::PropertySource::Curve(_))
            .then(|| {
                module
                    .property_source_values
                    .get(parameter)?
                    .iter()
                    .position(|value| value.source == source)
            })
            .flatten()
    });
    if let Some(index) = payload_index {
        let value = &mut module
            .property_source_values
            .get_mut(parameter)
            .expect("payload index came from this property")[index]
            .value;
        return match value {
            Value::Curve(curve) => Ok(curve),
            value => Err(CommandError::ParameterType {
                parameter: parameter.into(),
                expected: "curve",
                actual: value_type(value),
            }),
        };
    }
    match (&mut module.parameters, parameter) {
        (ModuleParameters::Appearance { size, .. }, "size") => Ok(size),
        (ModuleParameters::Appearance { opacity, .. }, "opacity") => Ok(opacity),
        (ModuleParameters::Custom(values), parameter) => match values.get_mut(parameter) {
            Some(Value::Curve(curve)) => Ok(curve),
            Some(value) => Err(CommandError::ParameterType {
                parameter: parameter.into(),
                expected: "curve",
                actual: value_type(value),
            }),
            None => Err(CommandError::UnknownParameter {
                parameter: parameter.into(),
            }),
        },
        _ => Err(CommandError::UnknownParameter {
            parameter: parameter.into(),
        }),
    }
}

fn module_gradient_mut<'a>(
    effect: &'a mut EffectAsset,
    emitter: EmitterId,
    module: ModuleId,
    parameter: &str,
) -> Result<&'a mut aestra_core::Gradient, CommandError> {
    let module = module_mut(effect, emitter, module)?;
    let payload_index = module.property_source(parameter).and_then(|source| {
        matches!(source, aestra_core::PropertySource::Gradient(_))
            .then(|| {
                module
                    .property_source_values
                    .get(parameter)?
                    .iter()
                    .position(|value| value.source == source)
            })
            .flatten()
    });
    if let Some(index) = payload_index {
        let value = &mut module
            .property_source_values
            .get_mut(parameter)
            .expect("payload index came from this property")[index]
            .value;
        return match value {
            Value::Gradient(gradient) => Ok(gradient),
            value => Err(CommandError::ParameterType {
                parameter: parameter.into(),
                expected: "gradient",
                actual: value_type(value),
            }),
        };
    }
    match (&mut module.parameters, parameter) {
        (ModuleParameters::Appearance { color, .. }, "color") => Ok(color),
        (ModuleParameters::Custom(values), parameter) => match values.get_mut(parameter) {
            Some(Value::Gradient(gradient)) => Ok(gradient),
            Some(value) => Err(CommandError::ParameterType {
                parameter: parameter.into(),
                expected: "gradient",
                actual: value_type(value),
            }),
            None => Err(CommandError::UnknownParameter {
                parameter: parameter.into(),
            }),
        },
        _ => Err(CommandError::UnknownParameter {
            parameter: parameter.into(),
        }),
    }
}

fn checked_insert<T>(
    items: &mut Vec<T>,
    index: usize,
    value: T,
    collection: &'static str,
) -> Result<(), CommandError> {
    if index > items.len() {
        return Err(CommandError::IndexOutOfBounds {
            collection,
            index,
            len: items.len(),
        });
    }
    items.insert(index, value);
    Ok(())
}

fn checked_remove<T>(
    items: &mut Vec<T>,
    index: usize,
    collection: &'static str,
) -> Result<T, CommandError> {
    if index >= items.len() {
        return Err(CommandError::IndexOutOfBounds {
            collection,
            index,
            len: items.len(),
        });
    }
    Ok(items.remove(index))
}

fn checked_replace<T: Copy>(
    items: &mut [T],
    index: usize,
    value: T,
    collection: &'static str,
) -> Result<T, CommandError> {
    let len = items.len();
    let Some(item) = items.get_mut(index) else {
        return Err(CommandError::IndexOutOfBounds {
            collection,
            index,
            len,
        });
    };
    Ok(std::mem::replace(item, value))
}

fn checked_move<T>(
    items: &mut Vec<T>,
    old_index: usize,
    new_index: usize,
    collection: &'static str,
) -> Result<(), CommandError> {
    if new_index >= items.len() {
        return Err(CommandError::IndexOutOfBounds {
            collection,
            index: new_index,
            len: items.len(),
        });
    }
    let item = items.remove(old_index);
    items.insert(new_index, item);
    Ok(())
}

fn not_found(kind: &'static str, id: &impl ToString) -> CommandError {
    CommandError::NotFound {
        kind,
        id: id.to_string(),
    }
}
