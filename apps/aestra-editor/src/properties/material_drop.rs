//! Renderer authoring drops. Source recipes/programs are never modified by assignment.
#[cfg(test)]
mod tests;

use super::*;
use crate::asset_browser::payload::{AssetPayload, AuthoringDropGuard};
use aestra_core::material::{MaterialDomain, MaterialInstance, MaterialProgramRef};
use aestra_project::ProjectAssetId;
use bevy::picking::events::{DragEnter, DragLeave};

#[derive(Component, Clone, Copy)]
pub(super) struct RendererDropTarget {
    pub effect: aestra_core::EffectId,
    pub renderer: RendererId,
}

struct Assignment {
    label: String,
    transaction: Option<EffectTransaction>,
}

fn renderer_domain(properties: &RendererProperties) -> Option<MaterialDomain> {
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
    if target.effect != session.effect.id {
        return Err("The effect changed; drop on its current renderer".into());
    }
    if session.pending_change.is_some() {
        return Err("Resolve the pending change before assigning a material".into());
    }
    let Some(ProjectAssetId::MaterialProgram(id)) = payload.resolve(catalog)? else {
        return Err("Drop a material program onto this renderer".into());
    };
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
    let program = catalog.material_program(id)?;
    if program.domain != expected {
        return Err(format!(
            "This renderer needs a {expected:?} material, not {:?}",
            program.domain
        ));
    }
    let functions = catalog.material_function_library()?;
    MaterialCompiler
        .compile_with_functions(&program, &functions)
        .map_err(|e| e.to_string())?;
    let reference = MaterialProgramRef::Project(id);
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

#[derive(Component)]
struct DropFeedback {
    source: AssetPayload,
    target: Entity,
}

pub(super) fn register(app: &mut App) {
    app.add_observer(hover)
        .add_observer(leave)
        .add_observer(drop_material)
        .add_systems(Update, cleanup);
}

fn source(
    entity: Entity,
    sources: &Query<&AssetPayload>,
    parents: &Query<&ChildOf>,
) -> Option<AssetPayload> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find_map(|entity| sources.get(entity).ok().cloned())
}
fn target(
    entity: Entity,
    targets: &Query<&RendererDropTarget>,
    parents: &Query<&ChildOf>,
) -> Option<(Entity, RendererDropTarget)> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find_map(|entity| {
            targets
                .get(entity)
                .ok()
                .copied()
                .map(|target| (entity, target))
        })
}

#[allow(clippy::too_many_arguments)]
fn hover(
    mut event: On<Pointer<DragEnter>>,
    sources: Query<&AssetPayload>,
    targets: Query<&RendererDropTarget>,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    guard: AuthoringDropGuard,
    feedback: Query<Entity, With<DropFeedback>>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Some(payload) = source(event.dragged, &sources, &parents) else {
        return;
    };
    let Some((entity, target)) = target(event.entity, &targets, &parents) else {
        return;
    };
    event.propagate(false);
    for entity in &feedback {
        commands.entity(entity).try_despawn();
    }
    let result = guard
        .check()
        .and_then(|()| plan(&payload, target, &catalog, &session));
    let color = if result.is_ok() {
        theme::ACCENT
    } else {
        Color::srgb(0.95, 0.3, 0.3)
    };
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
    commands.entity(entity).with_children(|parent| {
        parent
            .spawn((
                DropFeedback {
                    source: payload,
                    target: entity,
                },
                Pickable::IGNORE,
                GlobalZIndex(275),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::all(color),
            ))
            .with_child((
                Text::new(label),
                TextColor(theme::TEXT),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                Pickable::IGNORE,
                BackgroundColor(theme::PANEL),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    right: Val::Px(0.0),
                    max_width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(5.0)),
                    ..default()
                },
            ));
    });
}

fn leave(
    event: On<Pointer<DragLeave>>,
    parents: Query<&ChildOf>,
    feedback: Query<(Entity, &DropFeedback)>,
    mut commands: Commands,
) {
    for (entity, marker) in &feedback {
        if event.entity == marker.target
            || parents
                .iter_ancestors(event.entity)
                .any(|ancestor| ancestor == marker.target)
        {
            commands.entity(entity).try_despawn();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drop_material(
    mut event: On<Pointer<DragDrop>>,
    sources: Query<&AssetPayload>,
    targets: Query<&RendererDropTarget>,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    guard: AuthoringDropGuard,
    feedback: Query<Entity, With<DropFeedback>>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Some(payload) = source(event.dropped, &sources, &parents) else {
        return;
    };
    let Some((_, target)) = target(event.entity, &targets, &parents) else {
        return;
    };
    event.propagate(false);
    for entity in &feedback {
        commands.entity(entity).try_despawn();
    }
    match guard
        .check()
        .and_then(|()| plan(&payload, target, &catalog, &session))
    {
        Ok(plan) => {
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

fn cleanup(
    sources: Query<&AssetPayload>,
    feedback: Query<(Entity, &DropFeedback)>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    for (entity, marker) in &feedback {
        if keys
            .as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
            || catalog
                .as_ref()
                .is_none_or(|catalog| marker.source.resolve(catalog).is_err())
            || !sources.iter().any(|source| *source == marker.source)
        {
            commands.entity(entity).try_despawn();
        }
    }
}
