//! File-backed textures become effect-local references, never shared program defaults.
use super::asset_drop::Assignment;
use super::*;
use crate::asset_drop::{AssetPayload, resource::path_key};
use aestra_core::{AssetDefinition, AssetId};

#[derive(Clone, Copy)]
enum Slot {
    Sprite {
        renderer: RendererId,
        material: MaterialId,
    },
    Parameter {
        instance: MaterialId,
        program: MaterialProgramId,
        parameter: MaterialParameterId,
    },
}

#[derive(Component, Clone, Copy)]
pub(super) struct TextureDropTarget {
    effect: aestra_core::EffectId,
    slot: Slot,
}

impl TextureDropTarget {
    pub(super) fn sprite(
        session: &EditorSession,
        renderer: RendererId,
        material: MaterialId,
    ) -> Self {
        Self {
            effect: session.effect.id,
            slot: Slot::Sprite { renderer, material },
        }
    }
    pub(super) fn parameter(
        session: &EditorSession,
        instance: MaterialId,
        parameter: MaterialParameterId,
    ) -> Option<Self> {
        let program = session
            .effect
            .material_instances
            .iter()
            .find(|value| value.id == instance)?
            .program
            .id();
        Some(Self {
            effect: session.effect.id,
            slot: Slot::Parameter {
                instance,
                program,
                parameter,
            },
        })
    }
}

/// Keep the entire value row droppable, including labels and combo-button children.
pub(super) fn row(
    parent: &mut ChildSpawnerCommands,
    target: TextureDropTarget,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    crate::asset_drop::input_row(
        parent,
        target,
        "Drop a texture from Assets to assign it. Undo restores the previous input.",
        build,
    );
}

pub(super) fn plan(
    payload: &AssetPayload,
    target: TextureDropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    check_target(target, session)?;
    plan_source(payload, target, catalog, session)
}

fn check_target(target: TextureDropTarget, session: &EditorSession) -> Result<(), String> {
    if target.effect != session.effect.id {
        return Err("Effect changed; drop on its current texture input".into());
    }
    if session.pending_change.is_some() {
        return Err("Resolve the pending change before assigning a texture".into());
    }
    let material = match target.slot {
        Slot::Sprite { material, .. } => material,
        Slot::Parameter { instance, .. } => instance,
    };
    for emitter in &session.effect.emitters {
        for renderer in emitter
            .renderers
            .iter()
            .filter(|renderer| renderer.material == material)
        {
            if session.locks.is_locked(SemanticTarget::Emitter(emitter.id))
                || session
                    .locks
                    .is_locked(SemanticTarget::Renderer(renderer.id))
            {
                return Err("A renderer using this material is locked".into());
            }
        }
    }
    Ok(())
}

fn plan_source(
    payload: &AssetPayload,
    target: TextureDropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    let source = payload.texture_source(catalog)?;
    let path = source
        .relative_path
        .to_str()
        .ok_or("Texture path is not valid UTF-8")?
        .replace('\\', "/");
    let key = path_key(&path).ok_or("Texture must have a project-relative path")?;
    let selected = selected_asset(target, catalog, session);
    let existing = session
        .effect
        .assets
        .iter()
        .filter(|asset| {
            asset.kind == AssetKind::Texture && path_key(&asset.path).as_ref() == Some(&key)
        })
        .min_by_key(|asset| Some(asset.id) != selected);
    let name = source
        .path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Texture");
    let asset = existing
        .cloned()
        .unwrap_or_else(|| AssetDefinition::texture(name, path));
    let mut commands = Vec::new();
    if existing.is_none() {
        commands.push(EffectCommand::AddAsset {
            asset: asset.clone(),
            index: session.effect.assets.len(),
        });
    }
    plan_binding(
        target,
        Some(asset.id),
        &asset.name,
        catalog,
        session,
        commands,
    )
}

pub(super) fn allows_procedural(target: TextureDropTarget) -> bool {
    matches!(target.slot, Slot::Sprite { .. })
}

pub(super) fn plan_local(
    target: TextureDropTarget,
    asset: Option<AssetId>,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    check_target(target, session)?;
    let name = match asset {
        Some(id) => session
            .effect
            .assets
            .iter()
            .find(|a| a.id == id && a.kind == AssetKind::Texture)
            .ok_or("Texture is no longer registered")?
            .name
            .as_str(),
        None if allows_procedural(target) => "Procedural",
        None => return Err("This input requires a texture".into()),
    };
    plan_binding(target, asset, name, catalog, session, Vec::new())
}

fn plan_binding(
    target: TextureDropTarget,
    asset: Option<AssetId>,
    name: &str,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    mut commands: Vec<EffectCommand>,
) -> Result<Assignment, String> {
    let changed = match target.slot {
        Slot::Sprite { renderer, material } => {
            let renderer = session
                .effect
                .emitters
                .iter()
                .flat_map(|emitter| &emitter.renderers)
                .find(|value| value.id == renderer)
                .ok_or("Renderer no longer exists")?;
            if renderer.material != material
                || !matches!(renderer.properties, RendererProperties::Sprite)
            {
                return Err("This is no longer a sprite texture input; flipbooks need explicit atlas metadata".into());
            }
            let mut replacement = session
                .effect
                .materials
                .iter()
                .find(|value| value.id == material)
                .cloned()
                .ok_or("Sprite material no longer exists")?;
            let MaterialProperties::Sprite { texture, .. } = &mut replacement.properties;
            if *texture == asset {
                false
            } else {
                *texture = asset;
                commands.push(EffectCommand::SetMaterial {
                    id: material,
                    material: replacement,
                });
                true
            }
        }
        Slot::Parameter {
            instance,
            program,
            parameter,
        } => {
            let mut replacement = session
                .effect
                .material_instances
                .iter()
                .find(|value| value.id == instance)
                .cloned()
                .ok_or("Material instance no longer exists")?;
            if replacement.program.id() != program {
                return Err("Material binding changed; drop on its current input".into());
            }
            let definition = catalog.material_program(program)?;
            let descriptor = definition
                .parameters
                .iter()
                .find(|value| value.id == parameter)
                .ok_or("Material input no longer exists")?;
            if !matches!(descriptor.value_type, MaterialValueType::Texture2D(_)) {
                return Err("This material input does not accept a texture".into());
            }
            let value = MaterialParameterValue::Constant(MaterialValue::Texture2D(
                asset.ok_or("This input requires a texture")?,
            ));
            let current = replacement.values.get(&parameter).cloned().or_else(|| {
                descriptor
                    .default
                    .clone()
                    .map(MaterialParameterValue::Constant)
            });
            if current == Some(value.clone()) {
                false
            } else {
                replacement.values.insert(parameter, value);
                commands.push(EffectCommand::SetMaterialInstance {
                    id: instance,
                    instance: replacement,
                });
                true
            }
        }
    };
    if !changed {
        return Ok(Assignment {
            label: format!("{name} is already assigned"),
            transaction: None,
        });
    }
    let transaction = EffectTransaction::new(format!("Assign texture {name}"), commands);
    // Validate registration + binding together, including semantic locks and typed input rules.
    let mut candidate = session.effect.clone();
    aestra_authoring::CommandExecutor::execute(&mut candidate, &session.locks, &transaction)
        .map_err(|error| error.to_string())?;
    let programs = catalog.material_programs_for_effect(&candidate)?;
    MaterialAuthoringDocument::new(candidate, programs)
        .with_material_functions(catalog.material_functions()?)
        .validate()
        .map_err(|error| error.to_string())?;
    Ok(Assignment {
        label: format!("Assign texture {name}"),
        transaction: Some(transaction),
    })
}

fn selected_asset(
    target: TextureDropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Option<AssetId> {
    match target.slot {
        Slot::Sprite { material, .. } => {
            let material = session
                .effect
                .materials
                .iter()
                .find(|value| value.id == material)?;
            let MaterialProperties::Sprite { texture, .. } = material.properties;
            texture
        }
        Slot::Parameter {
            instance,
            program,
            parameter,
        } => {
            let instance = session
                .effect
                .material_instances
                .iter()
                .find(|value| value.id == instance)?;
            match instance.values.get(&parameter) {
                Some(MaterialParameterValue::Constant(MaterialValue::Texture2D(id))) => Some(*id),
                Some(_) => None,
                None => match catalog
                    .material_program(program)
                    .ok()?
                    .parameters
                    .iter()
                    .find(|value| value.id == parameter)?
                    .default
                {
                    Some(MaterialValue::Texture2D(id)) => Some(id),
                    _ => None,
                },
            }
        }
    }
}

pub(super) fn check_file(
    payload: &AssetPayload,
    catalog: &ProjectEffectCatalog,
) -> Result<(), String> {
    crate::asset_drop::resource::check_file(payload.texture_source(catalog)?, catalog)
}

#[cfg(test)]
mod tests;
