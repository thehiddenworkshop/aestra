//! The Grid2D fields supported by static thumbnails and hover previews.

use aestra_bevy_render::gpu::AestraFieldView;
use aestra_runtime::{CompiledEffect, FieldLayout};
use bevy::prelude::*;

use super::PresentedEffect;

pub(super) const MAX_EDGE: u32 = 128;

fn layouts(effect: &CompiledEffect) -> impl Iterator<Item = &FieldLayout> {
    effect
        .extension_stages
        .iter()
        .filter(|stage| stage.presentations.is_empty() && !stage.block.ops.is_empty())
        .flat_map(|stage| &stage.block.fields)
        .filter(|field| {
            field.dims[2] == 1 && field.bricks.is_none() && matches!(field.components, 1 | 2)
        })
}

fn layout(effect: &CompiledEffect) -> Option<&FieldLayout> {
    layouts(effect)
        .find(|field| field.components == 1)
        .or_else(|| layouts(effect).next())
}

pub(super) fn view(effect: &CompiledEffect) -> Option<AestraFieldView> {
    Some(AestraFieldView {
        resource: Some(layout(effect)?.resource.clone()),
        ..Default::default()
    })
}

pub(super) fn renders_field(effect: &CompiledEffect) -> bool {
    layout(effect).is_some()
}

pub(super) fn bounds(effect: &CompiledEffect) -> Result<Option<(Vec3, Vec3)>, String> {
    if layouts(effect).any(|field| field.dims[0].max(field.dims[1]) > MAX_EDGE) {
        return Err(format!("Grid2D thumbnail limit: {MAX_EDGE} cells per axis"));
    }
    let Some(field) = layout(effect) else {
        return Ok(None);
    };
    let [width, height, _] = field.dims;
    let center_z = field.origin[2] + field.cell_size * 0.5;
    let low = Vec3::new(field.origin[0], field.origin[1], center_z);
    let high = low + Vec3::new(width as f32, height as f32, 0.0) * field.cell_size;
    if !low.is_finite() || !high.is_finite() || (high - low).truncate().min_element() <= 0.0 {
        return Err("Non-finite or empty Grid2D bounds".into());
    }
    Ok(Some((low, high)))
}

pub(super) fn face_on(players: &[PresentedEffect]) -> bool {
    players.iter().any(|player| renders_field(player.effect()))
        && players.iter().all(|player| {
            player
                .effect()
                .all_extension_stages()
                .all(|stage| stage.presentations.is_empty())
        })
}
