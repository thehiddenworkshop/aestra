//! Production preset thumbnail composition, cache invalidation and status presentation.
use crate::material_graph::{
    MaterialGraphPreviewTarget, preview_pixels::render_material_graph_preview,
};
use crate::{ProjectEffectCatalog, feathers::tooltip::EditorTooltip, theme};
use aestra_compiler::{MaterialCompiler, MaterialPresetCatalog};
use aestra_core::{
    MaterialExpressionId, MaterialPresetId,
    material::{
        MaterialDomain, MaterialExpression, MaterialExpressionKind, MaterialInput, MaterialOutputs,
        MaterialPresetDescriptor, MaterialProgram, MaterialValue,
    },
};
use bevy::prelude::*;
use std::collections::BTreeMap;

pub(super) fn register(app: &mut App) {
    app.init_resource::<MaterialPresetPreviewState>()
        .add_systems(Update, rasterize_material_preset_previews);
}

#[derive(Debug, Clone)]
enum MaterialPresetPreviewStatus {
    Ready(Handle<Image>),
    Incompatible,
    Failed(String),
}

#[derive(Debug, Clone)]
struct MaterialPresetPreviewCache {
    fingerprint: u64,
    status: MaterialPresetPreviewStatus,
}

/// Cached deterministic thumbnails for project and built-in material presets.
///
/// The cache is intentionally independent from [`crate::EditorSession`]: generating or displaying a
/// preset thumbnail can never mutate the active material or participate in undo history.
#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialPresetPreviewState {
    cache: BTreeMap<MaterialPresetId, MaterialPresetPreviewCache>,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MaterialPresetPreviewRaster {
    pub(crate) preset: MaterialPresetId,
    pub(crate) status_label: Entity,
}

pub(crate) fn spawn_material_preset_preview(
    parent: &mut ChildSpawnerCommands,
    preset: MaterialPresetId,
    size: f32,
) -> Entity {
    let mut status_label = Entity::PLACEHOLDER;
    let mut preview = parent.spawn((
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            min_width: Val::Px(size),
            min_height: Val::Px(size),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        BorderColor::all(theme::BORDER),
        BackgroundColor(theme::PANEL_DARK),
        Pickable::IGNORE,
    ));
    let entity = preview.id();
    preview.with_children(|preview| {
        status_label = preview
            .spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Pickable::IGNORE,
            ))
            .id();
    });
    parent
        .commands()
        .entity(entity)
        .insert(MaterialPresetPreviewRaster {
            preset,
            status_label,
        });
    entity
}

fn rasterize_material_preset_previews(
    mut commands: Commands,
    requests: Query<(Entity, Ref<MaterialPresetPreviewRaster>)>,
    mut labels: Query<&mut Text>,
    catalog: Res<ProjectEffectCatalog>,
    mut previews: ResMut<MaterialPresetPreviewState>,
    mut images: ResMut<Assets<Image>>,
    mut previous_version: Local<Option<aestra_project::ProjectContentVersion>>,
) {
    // Snapshot publication may bypass Bevy resource change detection.
    let version = catalog.content_revision();
    let catalog_changed = previous_version.replace(version) != Some(version);
    if !catalog_changed && !requests.iter().any(|(_, request)| request.is_added()) {
        return;
    }
    let presets = catalog
        .material_preset_catalog()
        .unwrap_or_else(|_| MaterialCompiler.material_preset_catalog());
    for (entity, request) in &requests {
        if !catalog_changed && !request.is_added() {
            continue;
        }
        let Some(preset) = presets.get(request.preset) else {
            apply_material_preset_preview_status(
                &mut commands,
                &mut labels,
                entity,
                request.status_label,
                MaterialPresetPreviewStatus::Failed("Preset is unavailable".into()),
            );
            continue;
        };
        let fingerprint = material_preset_fingerprint(preset);
        let status = previews
            .cache
            .get(&request.preset)
            .filter(|cached| cached.fingerprint == fingerprint)
            .map(|cached| cached.status.clone())
            .unwrap_or_else(|| {
                let status = build_material_preset_preview(preset, &presets, &mut images);
                previews.cache.insert(
                    request.preset,
                    MaterialPresetPreviewCache {
                        fingerprint,
                        status: status.clone(),
                    },
                );
                status
            });
        apply_material_preset_preview_status(
            &mut commands,
            &mut labels,
            entity,
            request.status_label,
            status,
        );
    }
    previews
        .cache
        .retain(|preset, _| presets.get(*preset).is_some());
}

fn apply_material_preset_preview_status(
    commands: &mut Commands,
    labels: &mut Query<&mut Text>,
    entity: Entity,
    status_label: Entity,
    status: MaterialPresetPreviewStatus,
) {
    let Ok(mut label) = labels.get_mut(status_label) else {
        return;
    };
    match status {
        MaterialPresetPreviewStatus::Ready(image) => {
            label.0.clear();
            commands.entity(entity).remove::<EditorTooltip>().insert((
                ImageNode::new(image).with_mode(NodeImageMode::Stretch),
                BackgroundColor(Color::NONE),
            ));
        }
        MaterialPresetPreviewStatus::Incompatible => {
            label.0 = "N/A".into();
            commands
                .entity(entity)
                .remove::<EditorTooltip>()
                .remove::<ImageNode>()
                .insert(BackgroundColor(theme::PANEL_DARK));
        }
        MaterialPresetPreviewStatus::Failed(message) => {
            label.0 = "!".into();
            commands.entity(entity).remove::<ImageNode>().insert((
                BackgroundColor(theme::PANEL_DARK),
                EditorTooltip::titled("Preview failed", message),
            ));
        }
    }
}

fn material_preset_fingerprint(preset: &MaterialPresetDescriptor) -> u64 {
    let source =
        ron::ser::to_string(&preset.recipe).unwrap_or_else(|_| format!("{:?}", preset.recipe));
    source.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn build_material_preset_preview(
    preset: &MaterialPresetDescriptor,
    catalog: &MaterialPresetCatalog,
    images: &mut Assets<Image>,
) -> MaterialPresetPreviewStatus {
    let program = material_preset_base("Material preset preview", MaterialDomain::Sprite);
    let Some(target) = MaterialCompiler
        .stack_preset_targets_with_catalog(&program, catalog)
        .ok()
        .and_then(|targets| {
            targets
                .into_iter()
                .filter(|target| target.preset == preset.id)
                .max_by_key(|target| target.index)
        })
    else {
        return MaterialPresetPreviewStatus::Incompatible;
    };
    let replacement = match MaterialCompiler.plan_stack_insert_preset_with_catalog(
        &program,
        catalog,
        preset.id,
        target.index,
    ) {
        Ok(plan) => plan.replacement,
        Err(error) => return MaterialPresetPreviewStatus::Failed(error.to_string()),
    };
    // Presets use built-in functions (fresnel, etc.); the default library resolves them.
    let image = images.add(render_material_graph_preview(
        &replacement,
        None,
        &aestra_compiler::MaterialFunctionLibrary::default(),
        MaterialGraphPreviewTarget::Output,
        None,
    ));
    MaterialPresetPreviewStatus::Ready(image)
}

/// Resolves a preset to the material program it produces on the standard sprite base, so a
/// background thumbnail can render it through the shared material GPU path. Returns an error if
/// the preset is not compatible with the preview base or fails to plan.
pub(crate) fn resolve_preset_program(
    catalog: &MaterialPresetCatalog,
    preset: MaterialPresetId,
) -> Result<MaterialProgram, String> {
    let program = material_preset_base("Material preset preview", MaterialDomain::Sprite);
    let target = MaterialCompiler
        .stack_preset_targets_with_catalog(&program, catalog)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|target| target.preset == preset)
        .max_by_key(|target| target.index)
        .ok_or("Preset is not compatible with the preview base")?;
    MaterialCompiler
        .plan_stack_insert_preset_with_catalog(&program, catalog, preset, target.index)
        .map(|plan| plan.replacement)
        .map_err(|error| error.to_string())
}

/// Shared seed for preset thumbnails and creating a material from a preset.
/// Preset insertion needs a primary stack source; constant-only outputs have none.
pub(crate) fn material_preset_base(
    name: impl Into<String>,
    domain: MaterialDomain,
) -> MaterialProgram {
    let color = MaterialExpressionId::new();
    let uv = MaterialExpressionId::new();
    let center = MaterialExpressionId::new();
    let radius = MaterialExpressionId::new();
    let softness = MaterialExpressionId::new();
    let invert = MaterialExpressionId::new();
    let mask = MaterialExpressionId::new();
    let mut program = MaterialProgram::additive_sprite(name);
    program.domain = domain;
    program.expressions = vec![
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([
                0.72, 0.78, 0.92, 1.0,
            ])),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.5, 0.5])),
        },
        MaterialExpression {
            id: radius,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.42)),
        },
        MaterialExpression {
            id: softness,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.12)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: mask,
            kind: MaterialExpressionKind::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            },
        },
    ];
    program.outputs = MaterialOutputs {
        vertex_offset: None,
        color,
        alpha: mask,
    };
    program
}

#[cfg(test)]
#[path = "tests/preset_previews_integration.rs"]
mod integration_tests;
#[cfg(test)]
#[path = "tests/preset_previews.rs"]
mod tests;
