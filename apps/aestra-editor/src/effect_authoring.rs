//! Reusable-effect authoring commands, independent of the Library UI.
#[cfg(test)]
mod tests;
use crate::*;
use aestra_core::{
    AssetDefinition, AssetId, ChoreographyTrackId, CurveId, EffectAsset, EffectAssetRef,
    EffectClip, EffectClipId, EffectId, EffectParameter, Emitter, EmitterId, EmitterTransform,
    EventId, EventLink, FlipbookDefinition, GradientId, MaterialDefinition, MaterialId,
    MaterialInput, ModuleParameters, ParameterId, RendererProperties, SpriteColorSource, Value,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

#[derive(Debug)]
pub(crate) struct ReusableEffectPlan {
    pub(crate) effect: EffectAsset,
    selected: Vec<EmitterId>,
    clip_start: f32,
    clip_duration: f32,
}

pub(crate) fn reusable_effect_plan(
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

pub(crate) fn create_reusable_effect_from_emitters(
    emitters: &[EmitterId],
    name: &str,
    replace_selection: bool,
    catalog: &mut ProjectEffectCatalog,
    session: &mut EditorSession,
    localizer: &Localizer,
) -> Result<(), String> {
    let plan = reusable_effect_plan(&session.effect, emitters, name)?;
    let created = catalog
        .create_effect_source(&plan.effect)
        .map_err(|error| error.to_string())?;

    if replace_selection {
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

pub(crate) fn explode_effect_clip(
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
            RendererProperties::Sprite
            | RendererProperties::Ribbon { .. }
            | RendererProperties::Trail { .. } => {}
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
