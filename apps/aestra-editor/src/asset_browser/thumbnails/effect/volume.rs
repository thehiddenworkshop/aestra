//! Bounded job-owned fluid grids and generic framing of extension volume presentations.
use super::*;
use aestra_core::{EffectAsset, ModuleParameters, Value};
use aestra_runtime::{CompiledEffect, StagePresentation};

const GRID_EDGE: u32 = 32;
const STAGE_BYTES: u64 = 64 * 1024 * 1024;
const LIQUID_PARTICLES: u32 = 32_768;

pub(super) fn bound_fluid_preview(effect: &mut EffectAsset) {
    for module in effect
        .simulation_stages
        .iter_mut()
        .flat_map(|stage| &mut stage.modules)
        .chain(
            effect
                .emitters
                .iter_mut()
                .flat_map(|emitter| &mut emitter.modules),
        )
    {
        let ModuleParameters::Custom(values) = &mut module.parameters else {
            continue;
        };
        if module.module_type.0 == aestra_fluid::MODULE_GRID
            || module.module_type.0 == aestra_fluid::MODULE_LIQUID_GRID
        {
            let stride = if module.module_type.0 == aestra_fluid::MODULE_LIQUID_GRID {
                4
            } else {
                8
            };
            // Leave malformed inputs alone so compilation still diagnoses them. Preserve the
            // domain's world size and source placement, not the full-resolution allocation.
            if let (Some(Value::U32(resolution)), Some(Value::Scalar(cell_size))) =
                (values.get("resolution"), values.get("cell_size"))
                && *resolution > GRID_EDGE
                && resolution.is_multiple_of(stride)
                && cell_size.is_finite()
                && *cell_size > 0.0
            {
                let size = *cell_size * *resolution as f32 / GRID_EDGE as f32;
                values.insert("cell_size".into(), Value::Scalar(size));
                values.insert("resolution".into(), Value::U32(GRID_EDGE));
            }
            for (name, cap) in [("brick_budget", 64), ("pressure_iterations", 8)] {
                if let Some(Value::U32(value)) = values.get_mut(name) {
                    *value = (*value).min(cap);
                }
            }
            if module.module_type.0 == aestra_fluid::MODULE_LIQUID_GRID
                && let Some(Value::U32(value)) = values.get_mut("particle_budget")
            {
                *value = (*value).min(LIQUID_PARTICLES);
            }
        } else if module.module_type.0 == aestra_fluid::MODULE_VOLUME_LOOK
            || module.module_type.0 == aestra_fluid::MODULE_LIQUID_LOOK
        {
            for (name, cap) in [("steps", 64), ("shadow_steps", 2)] {
                if let Some(Value::U32(value)) = values.get_mut(name) {
                    *value = (*value).min(cap);
                }
            }
        }
    }
}

pub(super) fn check_budget(effect: &CompiledEffect) -> Result<(), String> {
    let mut bytes = 0u64;
    for stage in effect.all_extension_stages() {
        for resource in &stage.block.resources {
            bytes = bytes.saturating_add(resource.bytes);
        }
    }
    if bytes > STAGE_BYTES || effect.all_extension_stages().count() > 4 {
        return Err("Preview limit: four simulation stages and 64 MiB of stage buffers".into());
    }
    Ok(())
}

pub(super) fn bounds(effect: &CompiledEffect) -> Result<Option<(Vec3, Vec3)>, String> {
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for stage in effect.all_extension_stages() {
        for presentation in &stage.presentations {
            let StagePresentation::Volume(volume) = presentation;
            let layouts = volume.layouts(&stage.block)?;
            let layout = layouts[0];
            let low = Vec3::from_array(layout.origin);
            let high = low + Vec3::from_array(layout.dims.map(|dim| dim as f32)) * layout.cell_size;
            if !low.is_finite() || !high.is_finite() || (high - low).min_element() <= 0.0 {
                return Err("Non-finite or empty volume bounds".into());
            }
            bounds = Some(bounds.map_or((low, high), |(min, max)| (min.min(low), max.max(high))));
        }
    }
    Ok(bounds)
}
