//! Advects smoke density through a prescribed, time-varying XY velocity field.

use super::{
    GridModule, RESOURCE_GRID_2D_DENSITY, RESOURCE_GRID_2D_NEXT, RESOURCE_GRID_2D_VELOCITY,
    WORKGROUP, grid_effect, grid_stage, resource,
};
use crate::{fluid_module, number, vector};
use aestra_core::{
    CapabilityId, ComputeProgramId, EffectAsset, ModuleTypeId, PropertyBag, ResourceTypeId,
    StageTypeId, Value,
};
use aestra_extension::{
    CapabilityExpression, CapabilitySet, ComputeProgram, ExtensionRegistry, InputMetadata,
    ModuleMultiplicity, RegistryConflict, StageLowerer, StageLoweringInput, StageTypeDescriptor,
};
use aestra_runtime::{
    AESTRA_RESOURCE_FRAME, AESTRA_RESOURCE_STAGE_CONSTANTS, ComputeOp, CopyOp, ExecutionBlock,
    ExecutionOp, FrameConstants, Grid2dChannels, Grid2dLayout, QualityTier, ResourceAccess,
    ResourceLifetime,
};
use std::sync::Arc;

pub const STAGE_GRID_2D_SMOKE: &str = "org.example.aestra-fluid::stage/grid_2d_smoke";
pub const MODULE_GRID_2D_SMOKE: &str = "org.example.aestra-fluid::module/grid_2d_smoke";
const CAPABILITY: &str = "org.example.aestra-fluid::capability/grid_2d_smoke";
const PROGRAM: &str = "org.example.aestra-fluid::program/grid_2d_smoke";
const ENTRY: &str = "advect_smoke";
const MIN_RESOLUTION: u32 = 8;

pub(super) fn register(registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
    let capability = CapabilityId::new(CAPABILITY);
    registry.register_capability(capability.clone())?;
    let capabilities = CapabilitySet::new([capability]);
    let stage = StageTypeId::new(STAGE_GRID_2D_SMOKE);
    registry.register_stage(StageTypeDescriptor::gpu_only(
        stage.clone(),
        "Grid2D Smoke",
        capabilities.clone(),
    ))?;
    registry.register_module(
        fluid_module(
            MODULE_GRID_2D_SMOKE,
            "Smoke Advection",
            "Injects density and moves it through a time-varying XY airflow.",
            CapabilityExpression::AnyOf(capabilities),
        )
        .with_multiplicity(ModuleMultiplicity::Single)
        .with_inputs(vec![
            InputMetadata::new(
                "resolution",
                "Resolution",
                "Cells per side at authored quality.",
                Value::U32(96),
                number(1.0, MIN_RESOLUTION as f32, None),
            ),
            InputMetadata::new(
                "extent",
                "Grid Size",
                "Side length of the domain in effect space.",
                Value::Scalar(96.0),
                number(1.0, 0.01, None),
            )
            .with_unit("units"),
            InputMetadata::new(
                "airflow",
                "Airflow",
                "Base XY velocity. Positive Y lifts the smoke.",
                Value::Vec2([2.0, 15.0]),
                vector(),
            )
            .with_unit("units/s"),
            InputMetadata::new(
                "turbulence",
                "Airflow Variation",
                "Speed of local airflow changes across the grid.",
                Value::Scalar(9.0),
                number(0.5, 0.0, None),
            )
            .with_unit("units/s"),
            InputMetadata::new(
                "dissipation",
                "Dissipation",
                "Exponential decay rate of density.",
                Value::Scalar(0.36),
                number(0.05, 0.0, None),
            )
            .with_unit("1/s"),
            InputMetadata::new(
                "source_position",
                "Source Position",
                "XY center of the density source in effect space.",
                Value::Vec2([0.0, -28.0]),
                vector(),
            )
            .with_unit("units"),
            InputMetadata::new(
                "source_radius",
                "Source Radius",
                "Radius of the density source.",
                Value::Scalar(6.0),
                number(0.25, 0.01, None),
            )
            .with_unit("units"),
            InputMetadata::new(
                "density_rate",
                "Density Rate",
                "Density added per second at the center of the source.",
                Value::Scalar(3.2),
                number(0.1, 0.0, None),
            )
            .with_unit("1/s"),
        ]),
    )?;
    registry.lowering.register_module(
        ModuleTypeId::new(MODULE_GRID_2D_SMOKE),
        Arc::new(GridModule(ENTRY)),
    )?;
    registry
        .lowering
        .register_stage(stage, Arc::new(SmokeStage))?;
    registry.register_program(ComputeProgram {
        id: ComputeProgramId::new(PROGRAM),
        wgsl: include_str!("smoke.wgsl").into(),
        entry_points: vec![ENTRY.into()],
    })
}

/// An XY plume for previewing smoke advection and density injection in the editor.
pub fn grid_2d_smoke_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = grid_effect("Grid2D Smoke", 8.0);
    effect.simulation_stages.push(grid_stage(
        registry,
        "Grid2D Smoke",
        STAGE_GRID_2D_SMOKE,
        MODULE_GRID_2D_SMOKE,
    ));
    effect.extensions = registry.derive_requirements(&effect);
    effect
}

fn vec2(parameters: &PropertyBag, name: &str) -> Result<[f32; 2], String> {
    match parameters.get(name) {
        Some(Value::Vec2(value)) if value.iter().all(|v| v.is_finite()) => Ok(*value),
        _ => Err(format!("invalid Grid2D smoke {name}")),
    }
}

fn scalar(parameters: &PropertyBag, name: &str, min: f32) -> Result<f32, String> {
    parameters
        .get_f32(name)
        .filter(|v| v.is_finite() && *v >= min)
        .ok_or_else(|| format!("invalid Grid2D smoke {name}"))
}

struct SmokeStage;

impl StageLowerer for SmokeStage {
    fn lower(&self, input: &StageLoweringInput<'_>) -> Result<ExecutionBlock, String> {
        let params = match input.modules {
            [] => return Ok(ExecutionBlock::default()),
            [module] if module.module_type.0 == MODULE_GRID_2D_SMOKE => &module.parameters,
            _ => return Err("Grid2D Smoke expects one Smoke Advection module".into()),
        };
        let authored = params
            .get_u32("resolution")
            .filter(|&n| n >= MIN_RESOLUTION && n.checked_mul(n).is_some())
            .ok_or("invalid Grid2D smoke resolution")?;
        let extent = scalar(params, "extent", 0.01)?;
        let n = QualityTier::scale_count(authored, input.tier.resolution, 1, MIN_RESOLUTION);
        let cell_size = extent / n as f32;
        let grid = Grid2dLayout::new([n; 2], [extent * -0.5; 2], cell_size)
            .map_err(|error| error.to_string())?;
        let density = grid.field(
            ResourceTypeId::new(RESOURCE_GRID_2D_DENSITY),
            Grid2dChannels::Scalar,
        );
        let velocity = grid.field(
            ResourceTypeId::new(RESOURCE_GRID_2D_VELOCITY),
            Grid2dChannels::Vector2,
        );
        let density_bytes = density.cells() * density.cell_bytes();
        let flow = vec2(params, "airflow")?;
        let source = vec2(params, "source_position")?;
        let turbulence = scalar(params, "turbulence", 0.0)?;
        if flow.iter().any(|v| !(v.abs() + turbulence).is_finite()) {
            return Err("Grid2D smoke airflow exceeds the finite velocity range".into());
        }
        // Matches SmokeParams in smoke.wgsl. The vectors start at even words.
        let constants = std::iter::once(n)
            .chain(
                [
                    cell_size,
                    flow[0],
                    flow[1],
                    source[0],
                    source[1],
                    turbulence,
                    scalar(params, "dissipation", 0.0)?,
                    scalar(params, "source_radius", 0.01)?,
                    scalar(params, "density_rate", 0.0)?,
                ]
                .map(f32::to_bits),
            )
            .collect::<Vec<_>>();
        Ok(ExecutionBlock {
            resources: vec![
                resource(
                    RESOURCE_GRID_2D_DENSITY,
                    density_bytes,
                    ResourceLifetime::Persistent,
                ),
                resource(
                    RESOURCE_GRID_2D_NEXT,
                    density_bytes,
                    ResourceLifetime::Transient,
                ),
                resource(
                    RESOURCE_GRID_2D_VELOCITY,
                    velocity.cells() * velocity.cell_bytes(),
                    ResourceLifetime::Persistent,
                ),
                resource(
                    AESTRA_RESOURCE_STAGE_CONSTANTS,
                    constants.len() as u64 * 4,
                    ResourceLifetime::Persistent,
                ),
                resource(
                    AESTRA_RESOURCE_FRAME,
                    FrameConstants::BYTES,
                    ResourceLifetime::Persistent,
                ),
            ],
            ops: vec![
                ExecutionOp::Compute(ComputeOp {
                    name: "Advect Grid2D Smoke".into(),
                    program: Some(ComputeProgramId::new(PROGRAM)),
                    entry_point: ENTRY.into(),
                    accesses: vec![
                        ResourceAccess::read(RESOURCE_GRID_2D_DENSITY),
                        ResourceAccess::write(RESOURCE_GRID_2D_NEXT),
                        ResourceAccess::write(RESOURCE_GRID_2D_VELOCITY),
                        ResourceAccess::read(AESTRA_RESOURCE_STAGE_CONSTANTS),
                        ResourceAccess::read(AESTRA_RESOURCE_FRAME),
                    ],
                    dispatch: grid.dispatch([WORKGROUP; 2]),
                    indirect: None,
                }),
                ExecutionOp::Copy(CopyOp {
                    from: ResourceTypeId::new(RESOURCE_GRID_2D_NEXT),
                    to: density.resource.clone(),
                }),
            ],
            constants,
            fields: vec![density, velocity],
            ..Default::default()
        })
    }
}
