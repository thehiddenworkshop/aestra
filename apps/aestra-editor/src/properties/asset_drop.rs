//! Renderer authoring drops. Source recipes/programs are never modified by assignment.
mod preset_drop;
#[cfg(test)]
mod tests;

use super::*;
use crate::asset_drop::{AssetPayload, AuthoringDropGuard};
use aestra_core::material::{
    MaterialDomain, MaterialInstance, MaterialProgram, MaterialProgramRef,
};
use aestra_project::ProjectAssetId;
use bevy::picking::events::DragEnter;

#[derive(Component, Clone, Copy)]
pub(super) struct RendererDropTarget {
    pub effect: aestra_core::EffectId,
    pub renderer: RendererId,
}

pub(super) struct Assignment {
    pub label: String,
    pub transaction: Option<EffectTransaction>,
}

#[derive(Clone, Copy)]
pub(super) enum DropTarget {
    Renderer(RendererDropTarget),
    Texture(super::texture_drop::TextureDropTarget),
    Mesh(super::mesh_drop::MeshDropTarget),
    Material(RendererDropTarget),
}

#[derive(Event)]
pub(super) struct AssignAsset {
    pub payload: AssetPayload,
    pub target: DropTarget,
}
type DropTargets<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static RendererDropTarget>,
        Option<&'static super::texture_drop::TextureDropTarget>,
        Option<&'static super::mesh_drop::MeshDropTarget>,
        Option<&'static super::asset_picker::PickerField>,
    ),
>;

pub(super) fn plan_target(
    payload: &AssetPayload,
    target: DropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    payload.resolve(catalog)?;
    payload.check_document(session)?;
    if let Some(asset) = payload.virtual_asset()
        && !matches!(asset, crate::asset_drop::VirtualAsset::BuiltInPreset(_))
    {
        return super::virtual_drop::plan(asset, target, catalog, session);
    }
    if let Some(target) = mesh_target(payload, target, catalog, session)? {
        return super::mesh_drop::prepare(payload, target, catalog, session);
    }
    match target {
        DropTarget::Renderer(target) | DropTarget::Material(target) => {
            plan(payload, target, catalog, session)
        }
        DropTarget::Texture(target) => super::texture_drop::plan(payload, target, catalog, session),
        DropTarget::Mesh(_) => Err("Drop a mesh onto this input".into()),
    }
}

fn mesh_target(
    payload: &AssetPayload,
    target: DropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Option<super::mesh_drop::MeshDropTarget>, String> {
    if payload.virtual_asset().is_some() {
        return Ok(None);
    }
    match target {
        DropTarget::Mesh(target) => Ok(Some(target)),
        DropTarget::Renderer(target) if payload.resolve(catalog)?.is_none() => {
            super::mesh_drop::MeshDropTarget::capture(session, target).map(Some)
        }
        _ => Ok(None),
    }
}

pub(super) fn renderer_domain(properties: &RendererProperties) -> Option<MaterialDomain> {
    match properties {
        RendererProperties::Sprite | RendererProperties::Flipbook { .. } => {
            Some(MaterialDomain::Sprite)
        }
        RendererProperties::Ribbon { .. } | RendererProperties::Trail { .. } => {
            Some(MaterialDomain::Ribbon)
        }
        RendererProperties::Mesh { .. } => Some(MaterialDomain::Mesh),
        RendererProperties::Custom(_) => None,
    }
}

fn plan(
    payload: &AssetPayload,
    target: RendererDropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    match payload.resolve(catalog)? {
        Some(ProjectAssetId::MaterialProgram(id)) => {
            plan_program(&catalog.material_program(id)?, target, catalog, session)
        }
        Some(ProjectAssetId::MaterialPreset(_)) => {
            let program = preset_drop::prepare(payload, target, catalog, session)?;
            let mut assignment = plan_program(&program, target, catalog, session)?;
            assignment.label = format!("Create material from {}…", program.name);
            Ok(assignment)
        }
        _ => Err("Drop a material or material preset onto this renderer".into()),
    }
}

fn plan_program(
    program: &MaterialProgram,
    target: RendererDropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    if target.effect != session.effect.id {
        return Err("The effect changed; drop on its current renderer".into());
    }
    if session.pending_change.is_some() {
        return Err("Resolve the pending change before assigning a material".into());
    }
    let (emitter, renderer) = session
        .effect
        .emitters
        .iter()
        .find_map(|emitter| {
            emitter
                .renderers
                .iter()
                .find(|renderer| renderer.id == target.renderer)
                .map(|renderer| (emitter.id, renderer))
        })
        .ok_or("Renderer no longer exists")?;
    let expected = renderer_domain(&renderer.properties)
        .ok_or("This renderer does not support material assignment")?;
    if program.domain != expected {
        return Err(format!(
            "This renderer needs a {expected:?} material, not {:?}",
            program.domain
        ));
    }
    let functions = catalog.material_function_library()?;
    MaterialCompiler
        .compile_with_functions(program, &functions)
        .map_err(|e| e.to_string())?;
    let reference = MaterialProgramRef::Project(program.id);
    if session
        .effect
        .material_instances
        .iter()
        .any(|instance| instance.id == renderer.material && instance.program == reference)
    {
        return Ok(Assignment {
            label: format!("{} is already assigned", program.name),
            transaction: None,
        });
    }
    // Reuse only a default instance: never silently adopt another renderer's overrides.
    let reused = session.effect.material_instances.iter().find(|instance| {
        instance.program == reference
            && instance.values.is_empty()
            && instance.render_state == program.render_state_policy.default
    });
    let mut commands = Vec::new();
    let material = if let Some(instance) = reused {
        instance.id
    } else {
        let instance = MaterialInstance {
            id: MaterialId::new(),
            program: reference,
            values: Default::default(),
            render_state: program.render_state_policy.default,
        };
        let id = instance.id;
        commands.push(EffectCommand::AddMaterialInstance {
            instance,
            index: session.effect.material_instances.len(),
        });
        id
    };
    commands.push(EffectCommand::SetRendererMaterial {
        emitter,
        renderer: target.renderer,
        material,
    });
    let transaction = EffectTransaction::new(format!("Assign {}", program.name), commands);
    // Validate the complete candidate (including locks) without touching session/history.
    let mut candidate = session.effect.clone();
    aestra_authoring::CommandExecutor::execute(&mut candidate, &session.locks, &transaction)
        .map_err(|e| e.to_string())?;
    let mut programs = catalog.material_programs_for_effect(&session.effect)?;
    if !programs.iter().any(|p| p.id == program.id) {
        programs.push(program.clone());
    }
    let mut document = MaterialAuthoringDocument::new(candidate, programs);
    document.material_functions = catalog.material_functions()?;
    document
        .validate()
        .map_err(|report| format!("Material assignment is invalid: {report:?}"))?;
    Ok(Assignment {
        label: format!("Assign {}", program.name),
        transaction: Some(transaction),
    })
}

type DropFeedback = crate::asset_drop::Feedback<DropTarget>;

pub(super) fn plan_local_material(
    target: RendererDropTarget,
    material: MaterialId,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    if target.effect != session.effect.id || session.pending_change.is_some() {
        return Err("Effect changed or has a pending edit".into());
    }
    let (emitter, renderer) = session
        .effect
        .emitters
        .iter()
        .find_map(|emitter| {
            emitter
                .renderers
                .iter()
                .find(|r| r.id == target.renderer)
                .map(|r| (emitter.id, r))
        })
        .ok_or("Renderer no longer exists")?;
    let expected = renderer_domain(&renderer.properties).ok_or("Unsupported renderer")?;
    let programs = catalog.material_programs_for_effect(&session.effect)?;
    let compatible = if session.effect.materials.iter().any(|m| m.id == material) {
        matches!(
            renderer.properties,
            RendererProperties::Sprite
                | RendererProperties::Flipbook { .. }
                | RendererProperties::Trail { .. }
        )
    } else {
        session
            .effect
            .material_instances
            .iter()
            .find(|m| m.id == material)
            .and_then(|instance| programs.iter().find(|p| p.id == instance.program.id()))
            .is_some_and(|program| program.domain == expected)
    };
    if !compatible {
        return Err("Material is missing or incompatible with this renderer".into());
    }
    let transaction = EffectTransaction::single(
        "Assign local material",
        EffectCommand::SetRendererMaterial {
            emitter,
            renderer: renderer.id,
            material,
        },
    );
    let mut candidate = session.effect.clone();
    aestra_authoring::CommandExecutor::execute(&mut candidate, &session.locks, &transaction)
        .map_err(|e| e.to_string())?;
    MaterialAuthoringDocument::new(candidate, programs)
        .with_material_functions(catalog.material_functions()?)
        .validate()
        .map_err(|e| e.to_string())?;
    Ok(Assignment {
        label: "Assign local material".into(),
        transaction: (renderer.material != material).then_some(transaction),
    })
}

pub(super) fn register(app: &mut App) {
    preset_drop::register(app);
    super::mesh_drop::register(app);
    super::asset_picker::register(app);
    crate::asset_drop::register_feedback::<DropTarget>(app);
    app.add_observer(hover)
        .add_observer(drop_asset)
        .add_observer(assign_asset);
}

fn target(entity: Entity, targets: &DropTargets) -> Option<DropTarget> {
    let (renderer, texture, mesh, picker) = targets.get(entity).ok()?;
    if let Some(picker) = picker {
        return Some(picker.0);
    }
    if let Some(mesh) = mesh {
        return Some(DropTarget::Mesh(*mesh));
    }
    texture
        .copied()
        .map(DropTarget::Texture)
        .or_else(|| renderer.copied().map(DropTarget::Renderer))
}

#[allow(clippy::too_many_arguments)]
fn hover(
    mut event: On<Pointer<DragEnter>>,
    sources: Query<&AssetPayload>,
    targets: DropTargets,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    guard: AuthoringDropGuard,
    feedback: Query<Entity, With<DropFeedback>>,
    mut commands: Commands,
) {
    let Some((payload, entity, target)) = crate::asset_drop::resolve(
        event.button,
        event.dragged,
        event.entity,
        &sources,
        &parents,
        |entity| target(entity, &targets),
    ) else {
        return;
    };
    event.propagate(false);
    crate::asset_drop::clear_feedback(&feedback, &mut commands);
    let result = guard
        .check()
        .and_then(|()| plan_target(&payload, target, &catalog, &session));
    let accepted = result.is_ok();
    let label = result.map_or_else(
        |error| error,
        |plan| {
            if plan.transaction.is_some() {
                format!("Release to {}", plan.label)
            } else {
                plan.label
            }
        },
    );
    crate::asset_drop::show_feedback::<DropTarget>(&mut commands, entity, payload, label, accepted);
}

#[allow(clippy::too_many_arguments)]
fn drop_asset(
    mut event: On<Pointer<DragDrop>>,
    sources: Query<&AssetPayload>,
    targets: DropTargets,
    parents: Query<&ChildOf>,
    feedback: Query<Entity, With<DropFeedback>>,
    mut commands: Commands,
) {
    let Some((payload, _, target)) = crate::asset_drop::resolve(
        event.button,
        event.dropped,
        event.entity,
        &sources,
        &parents,
        |entity| target(entity, &targets),
    ) else {
        return;
    };
    event.propagate(false);
    crate::asset_drop::clear_feedback(&feedback, &mut commands);
    commands.trigger(AssignAsset { payload, target });
}

fn assign_asset(
    event: On<AssignAsset>,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    guard: AuthoringDropGuard,
    mut commands: Commands,
) {
    let payload = event.payload.clone();
    let target = event.target;
    match guard.check_release().and_then(|()| {
        if matches!(target, DropTarget::Texture(_)) && payload.virtual_asset().is_none() {
            super::texture_drop::check_file(&payload, &catalog)?;
        }
        plan_target(&payload, target, &catalog, &session)
    }) {
        Ok(plan) => {
            if let Ok(Some(target)) = mesh_target(&payload, target, &catalog, &session) {
                commands.trigger(super::mesh_drop::OpenMeshDrop { payload, target });
                return;
            }
            if let DropTarget::Renderer(target) | DropTarget::Material(target) = target
                && matches!(
                    payload.resolve(&catalog),
                    Ok(Some(ProjectAssetId::MaterialPreset(_)))
                )
            {
                commands.trigger(preset_drop::OpenPresetDrop { payload, target });
                return;
            }
            if let Some(transaction) = plan.transaction {
                if session.execute_transaction(transaction, true) {
                    session.material_history_active = false;
                }
            } else {
                session.status = plan.label;
            }
        }
        Err(error) => session.status = format!("Material drop rejected: {error}"),
    }
}
