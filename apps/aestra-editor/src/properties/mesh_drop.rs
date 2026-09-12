//! Mesh assignment changes only geometry; it never imports a scene or replaces materials.
mod prompt;

use super::asset_drop::{Assignment, RendererDropTarget};
use super::*;
use crate::asset_drop::{AssetPayload, resource};
use aestra_core::material::MaterialProgram;
use aestra_core::{AssetDefinition, AssetId};
use aestra_project::ProjectMeshPrimitive;
pub(super) use prompt::{OpenMeshDrop, OpenMeshRenderer, register};

#[derive(Clone, Copy)]
enum Target {
    Existing(MeshDropTarget),
    Create(aestra_core::EmitterId),
}

impl Target {
    fn check(self, session: &EditorSession) -> Result<(), String> {
        match self {
            Self::Existing(target) => target.check(session),
            Self::Create(emitter) => {
                if session.pending_change.is_some()
                    || !session
                        .effect
                        .emitters
                        .iter()
                        .any(|item| item.id == emitter)
                    || session.locks.is_locked(SemanticTarget::Emitter(emitter))
                {
                    return Err("Emitter changed, is locked, or has a pending edit".into());
                }
                Ok(())
            }
        }
    }
}

#[derive(Component, Clone, Copy)]
pub(super) struct MeshDropTarget {
    effect: aestra_core::EffectId,
    renderer: RendererId,
    asset: AssetId,
}

impl MeshDropTarget {
    pub(super) fn capture(
        session: &EditorSession,
        target: RendererDropTarget,
    ) -> Result<Self, String> {
        let renderer = session
            .effect
            .emitters
            .iter()
            .flat_map(|emitter| &emitter.renderers)
            .find(|renderer| renderer.id == target.renderer)
            .ok_or("Renderer no longer exists")?;
        let RendererProperties::Mesh { asset } = renderer.properties else {
            return Err("Drop geometry onto a mesh renderer, not this renderer type".into());
        };
        Ok(Self {
            effect: target.effect,
            renderer: target.renderer,
            asset,
        })
    }

    fn check(&self, session: &EditorSession) -> Result<(), String> {
        if session.effect.id != self.effect || session.pending_change.is_some() {
            return Err(
                "Effect changed or has a pending edit; drop on its current mesh input".into(),
            );
        }
        let (emitter, renderer) = session
            .effect
            .emitters
            .iter()
            .find_map(|emitter| {
                emitter
                    .renderers
                    .iter()
                    .find(|renderer| renderer.id == self.renderer)
                    .map(|renderer| (emitter, renderer))
            })
            .ok_or("Renderer no longer exists")?;
        if renderer.properties != (RendererProperties::Mesh { asset: self.asset }) {
            return Err("Mesh binding changed; drop on the current input".into());
        }
        if session.locks.is_locked(SemanticTarget::Emitter(emitter.id))
            || session
                .locks
                .is_locked(SemanticTarget::Renderer(renderer.id))
        {
            return Err("The mesh renderer or its emitter is locked".into());
        }
        Ok(())
    }
}

pub(super) fn prepare(
    payload: &AssetPayload,
    target: MeshDropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    target.check(session)?;
    let source = payload.mesh_source(catalog)?;
    Ok(Assignment {
        label: format!(
            "Release to assign mesh {} (choose a primitive if needed)",
            source
                .path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("Mesh")
        ),
        transaction: None,
    })
}

fn resource_key(path: &str) -> Option<String> {
    let (file, label) = path.split_once('#')?;
    // Loader labels are case-sensitive even on Windows.
    Some(format!("{}#{label}", resource::path_key(file)?))
}

fn plan(
    payload: &AssetPayload,
    target: MeshDropTarget,
    primitive: &ProjectMeshPrimitive,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    plan_target(
        payload,
        Target::Existing(target),
        primitive,
        catalog,
        session,
    )
}

fn plan_target(
    payload: &AssetPayload,
    target: Target,
    primitive: &ProjectMeshPrimitive,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    target.check(session)?;
    let source = payload.mesh_source(catalog)?;
    let label = primitive.loader_label();
    let path = format!(
        "{}#{label}",
        source
            .relative_path
            .to_str()
            .ok_or("Mesh path is not valid UTF-8")?
            .replace('\\', "/")
    );
    let key = resource_key(&path).ok_or("Mesh must have a project-relative path")?;
    let existing = session
        .effect
        .assets
        .iter()
        .filter(|asset| {
            asset.kind == AssetKind::Mesh && resource_key(&asset.path).as_ref() == Some(&key)
        })
        .min_by_key(
            |asset| !matches!(target, Target::Existing(target) if asset.id == target.asset),
        );
    let name = format!(
        "{} · {label}",
        source
            .path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Mesh")
    );
    let asset = existing.cloned().unwrap_or_else(|| AssetDefinition {
        id: AssetId::new(),
        name,
        kind: AssetKind::Mesh,
        path,
    });
    if matches!(target, Target::Existing(target) if asset.id == target.asset) {
        return Ok(Assignment {
            label: format!("{} is already assigned", asset.name),
            transaction: None,
        });
    }
    let mut commands = Vec::new();
    if existing.is_none() {
        commands.push(EffectCommand::AddAsset {
            asset: asset.clone(),
            index: session.effect.assets.len(),
        });
    }
    match target {
        Target::Existing(target) => commands.push(EffectCommand::SetRendererProperties {
            emitter: session
                .effect
                .emitters
                .iter()
                .find(|emitter| {
                    emitter
                        .renderers
                        .iter()
                        .any(|renderer| renderer.id == target.renderer)
                })
                .ok_or("Renderer no longer exists")?
                .id,
            renderer: target.renderer,
            properties: RendererProperties::Mesh { asset: asset.id },
        }),
        Target::Create(emitter) => {
            // The built-in mesh program has no external textures or project-source dependency.
            // Give each renderer its own local instance, without borrowing other overrides.
            let reference = aestra_core::material::MaterialProgramRef::BuiltIn(
                MaterialProgram::DEFAULT_MESH_ID,
            );
            let program = MaterialProgram::built_in(reference).unwrap();
            let instance = aestra_core::material::MaterialInstance {
                id: MaterialId::new(),
                program: reference,
                values: default(),
                render_state: program.render_state_policy.default,
            };
            let mut renderer = aestra_core::RendererInstance::sprite(instance.id);
            renderer.renderer_type = aestra_core::RendererTypeId::new(aestra_core::RENDERER_MESH);
            renderer.properties = RendererProperties::Mesh { asset: asset.id };
            commands.push(EffectCommand::AddMaterialInstance {
                instance,
                index: session.effect.material_instances.len(),
            });
            commands.push(EffectCommand::AddRenderer {
                emitter,
                renderer,
                index: session
                    .effect
                    .emitters
                    .iter()
                    .find(|item| item.id == emitter)
                    .unwrap()
                    .renderers
                    .len(),
            });
        }
    }
    let action = if matches!(target, Target::Create(_)) {
        "Add mesh renderer"
    } else {
        "Assign mesh"
    };
    let transaction = EffectTransaction::new(format!("{action} {}", asset.name), commands);
    let mut candidate = session.effect.clone();
    aestra_authoring::CommandExecutor::execute(&mut candidate, &session.locks, &transaction)
        .map_err(|error| error.to_string())?;
    let programs = catalog.material_programs_for_effect(&candidate)?;
    MaterialAuthoringDocument::new(candidate, programs)
        .with_material_functions(catalog.material_functions()?)
        .validate()
        .map_err(|error| error.to_string())?;
    Ok(Assignment {
        label: format!("{action} {}", asset.name),
        transaction: Some(transaction),
    })
}

fn inspect(
    payload: &AssetPayload,
    catalog: &ProjectEffectCatalog,
) -> Result<Vec<ProjectMeshPrimitive>, String> {
    use std::io::Read;
    let source = payload.mesh_source(catalog)?;
    resource::check_file(source, catalog)?;
    const LIMIT: u64 = 64 * 1024 * 1024;
    if source
        .metadata
        .as_ref()
        .is_none_or(|metadata| metadata.bytes > LIMIT)
    {
        return Err(
            "Mesh inspection supports files up to 64 MiB; split or reduce this file".into(),
        );
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&source.path)
        .map_err(|error| error.to_string())?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("Mesh grew while reading; refresh Assets".into());
    }
    resource::check_file(source, catalog)?;
    aestra_project::inspect_mesh_primitives(&bytes)
}

pub(super) fn spawn_input(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    renderer: &aestra_core::RendererInstance,
) {
    let target = RendererDropTarget {
        effect: session.effect.id,
        renderer: renderer.id,
    };
    let Ok(target) = MeshDropTarget::capture(session, target) else {
        return;
    };
    let current = session
        .effect
        .assets
        .iter()
        .find(|asset| asset.id == target.asset)
        .map_or("Missing mesh — drop from Assets", |asset| {
            asset.name.as_str()
        });
    crate::asset_drop::input_row(
        parent,
        target,
        "Drop a glTF/GLB mesh from Assets. Multiple primitives open a chooser; materials stay unchanged.",
        |row| {
            super::asset_picker::row(
                row,
                super::asset_drop::DropTarget::Mesh(target),
                "Mesh",
                current,
            )
        },
    );
}
