//! Virtual sources reuse the same field validation and transaction route as project drops.
use super::asset_drop::{Assignment, DropTarget, RendererDropTarget};
use super::*;
use crate::asset_drop::VirtualAsset;

#[cfg(test)]
mod tests;

pub(super) fn plan(
    asset: VirtualAsset,
    target: DropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    match (asset, target) {
        (
            VirtualAsset::Material(id),
            DropTarget::Renderer(target) | DropTarget::Material(target),
        ) => super::asset_drop::plan_local_material(target, id, catalog, session),
        (VirtualAsset::Texture(id), DropTarget::Texture(target)) => {
            super::texture_drop::plan_local(target, Some(id), catalog, session)
        }
        (VirtualAsset::Mesh(id), DropTarget::Mesh(target)) => {
            target.check(session)?;
            geometry(
                asset,
                target.renderer(),
                RendererProperties::Mesh { asset: id },
                catalog,
                session,
            )
        }
        (VirtualAsset::Mesh(id), DropTarget::Renderer(target)) => geometry(
            asset,
            target,
            RendererProperties::Mesh { asset: id },
            catalog,
            session,
        ),
        (VirtualAsset::Flipbook(id), DropTarget::Renderer(target)) => {
            let mut properties = session
                .effect
                .emitters
                .iter()
                .flat_map(|e| &e.renderers)
                .find(|r| r.id == target.renderer)
                .ok_or("Renderer no longer exists")?
                .properties
                .clone();
            let RendererProperties::Flipbook { flipbook, .. } = &mut properties else {
                return Err("Drop a flipbook onto a Flipbook renderer".into());
            };
            *flipbook = id;
            geometry(asset, target, properties, catalog, session)
        }
        _ => Err("This resource is incompatible with this input".into()),
    }
}

fn geometry(
    asset: VirtualAsset,
    target: RendererDropTarget,
    properties: RendererProperties,
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
    let name = match (asset, &renderer.properties) {
        (VirtualAsset::Mesh(id), RendererProperties::Mesh { .. }) => session
            .effect
            .assets
            .iter()
            .find(|a| a.id == id && a.kind == AssetKind::Mesh)
            .map(|a| a.name.clone()),
        (VirtualAsset::Flipbook(id), RendererProperties::Flipbook { .. }) => session
            .effect
            .flipbooks
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.name.clone()),
        _ => return Err("Drop this resource onto a matching renderer type".into()),
    }
    .ok_or("Resource no longer exists")?;
    let transaction = EffectTransaction::single(
        format!("Assign {name}"),
        EffectCommand::SetRendererProperties {
            emitter,
            renderer: renderer.id,
            properties: properties.clone(),
        },
    );
    let mut candidate = session.effect.clone();
    aestra_authoring::CommandExecutor::execute(&mut candidate, &session.locks, &transaction)
        .map_err(|e| e.to_string())?;
    let mut document = MaterialAuthoringDocument::new(
        candidate,
        catalog.material_programs_for_effect(&session.effect)?,
    );
    document.material_functions = catalog.material_functions()?;
    document
        .validate()
        .map_err(|report| format!("Resource assignment is invalid: {report:?}"))?;
    Ok(Assignment {
        label: format!("Assign {name}"),
        transaction: (renderer.properties != properties).then_some(transaction),
    })
}
