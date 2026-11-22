//! Reference Grid2D stages using the standard extension and field contracts.

mod smoke;
pub use smoke::{MODULE_GRID_2D_SMOKE, STAGE_GRID_2D_SMOKE, grid_2d_smoke_effect};

use aestra_core::{
    AESTRA_DOMAIN_GRID_2D, CapabilityId, ComputeProgramId, DomainTypeId, EffectAsset,
    EffectDimension, EffectPlaybackMode, EffectSimulationStage, Emitter, EmitterShape,
    ModuleInstance, ModuleParameters, ModuleTypeId, PropertyBag, ResourceTypeId, ScalarRange,
    StageKind, StageTypeId, Value,
};
use aestra_extension::{
    CapabilityExpression, CapabilitySet, ComputeProgram, ExtensionRegistry, InputMetadata,
    ModuleLowerer, ModuleMultiplicity, RegistryConflict, ResourceTypeDescriptor, StageLowerer,
    StageLoweringInput, StageTypeDescriptor,
};
use aestra_runtime::{
    AESTRA_RESOURCE_FRAME, AESTRA_RESOURCE_STAGE_CONSTANTS, ComputeOp, CopyOp, ExecutionBlock,
    ExecutionOp, ExtensionModulePlan, FrameConstants, Grid2dChannels, Grid2dLayout, ResourceAccess,
    ResourceDescriptor, ResourceLifetime,
};
use std::{num::NonZeroU32, sync::Arc};

pub const STAGE_GRID_2D_DIFFUSION: &str = "org.example.aestra-fluid::stage/grid_2d_diffusion";
pub const MODULE_GRID_2D_DIFFUSION: &str = "org.example.aestra-fluid::module/grid_2d_diffusion";
pub const RESOURCE_GRID_2D_DENSITY: &str = "org.example.aestra-fluid::resource/grid_2d_density";
pub const STAGE_GRID_2D_FLOW: &str = "org.example.aestra-fluid::stage/grid_2d_flow";
pub const MODULE_GRID_2D_FLOW: &str = "org.example.aestra-fluid::module/grid_2d_flow";
pub const RESOURCE_GRID_2D_VELOCITY: &str = "org.example.aestra-fluid::resource/grid_2d_velocity";
const RESOURCE_GRID_2D_NEXT: &str = "org.example.aestra-fluid::resource/grid_2d_density_next";
const CAPABILITY_GRID_2D_DIFFUSION: &str = "org.example.aestra-fluid::capability/grid_2d_diffusion";
const DIFFUSION_PROGRAM: &str = "org.example.aestra-fluid::program/grid_2d_diffusion";
const DIFFUSION_ENTRY: &str = "diffuse_grid_2d";
const CAPABILITY_GRID_2D_FLOW: &str = "org.example.aestra-fluid::capability/grid_2d_flow";
const FLOW_PROGRAM: &str = "org.example.aestra-fluid::program/grid_2d_flow";
const FLOW_ENTRY: &str = "write_grid_2d_velocity";
const WORKGROUP: NonZeroU32 = NonZeroU32::new(8).unwrap();

pub(super) fn register(registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
    register_diffusion(registry)?;
    register_flow(registry)?;
    smoke::register(registry)
}

fn register_diffusion(registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
    let capability = CapabilityId::new(CAPABILITY_GRID_2D_DIFFUSION);
    registry.register_capability(capability.clone())?;
    for (id, name, lifetime) in [
        (
            RESOURCE_GRID_2D_DENSITY,
            "Grid2D Density",
            ResourceLifetime::Persistent,
        ),
        (
            RESOURCE_GRID_2D_NEXT,
            "Grid2D Density Scratch",
            ResourceLifetime::Transient,
        ),
    ] {
        registry.resources.register(ResourceTypeDescriptor {
            type_id: ResourceTypeId::new(id),
            display_name: name.into(),
            domain: DomainTypeId::new(AESTRA_DOMAIN_GRID_2D),
            lifetime,
        })?;
    }
    let stage_type = StageTypeId::new(STAGE_GRID_2D_DIFFUSION);
    let capabilities = CapabilitySet::new([capability]);
    registry.register_stage(StageTypeDescriptor::gpu_only(
        stage_type.clone(),
        "Grid2D Diffusion",
        capabilities.clone(),
    ))?;
    let module = super::fluid_module(
        MODULE_GRID_2D_DIFFUSION,
        "Diffusion",
        "Spreads a pulse of density through an XY grid with wraparound edges.",
        CapabilityExpression::AnyOf(capabilities),
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(
        grid_inputs()
            .into_iter()
            .chain([InputMetadata::new(
                "rate",
                "Diffusion Rate",
                "How much density transfers per step.",
                Value::Scalar(0.2),
                super::number(0.01, 0.0, Some(0.25)),
            )])
            .collect(),
    );
    registry.register_module(module)?;
    registry.lowering.register_module(
        ModuleTypeId::new(MODULE_GRID_2D_DIFFUSION),
        Arc::new(GridModule(DIFFUSION_ENTRY)),
    )?;
    registry
        .lowering
        .register_stage(stage_type, Arc::new(DiffusionStage))?;
    registry.register_program(ComputeProgram {
        id: ComputeProgramId::new(DIFFUSION_PROGRAM),
        wgsl: include_str!("grid_2d_diffusion.wgsl").into(),
        entry_points: vec![DIFFUSION_ENTRY.into()],
    })
}

fn grid_inputs() -> [InputMetadata; 3] {
    [
        InputMetadata::new(
            "width",
            "Width",
            "Cells along X.",
            Value::U32(40),
            super::number(1.0, 1.0, None),
        ),
        InputMetadata::new(
            "height",
            "Height",
            "Cells along Y.",
            Value::U32(28),
            super::number(1.0, 1.0, None),
        ),
        InputMetadata::new(
            "cell_size",
            "Cell Size",
            "The side length of one cell.",
            Value::Scalar(2.0),
            super::number(0.1, 0.001, None),
        )
        .with_unit("units"),
    ]
}

fn register_flow(registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
    let capability = CapabilityId::new(CAPABILITY_GRID_2D_FLOW);
    registry.register_capability(capability.clone())?;
    registry.resources.register(ResourceTypeDescriptor {
        type_id: ResourceTypeId::new(RESOURCE_GRID_2D_VELOCITY),
        display_name: "Grid2D Velocity".into(),
        domain: DomainTypeId::new(AESTRA_DOMAIN_GRID_2D),
        lifetime: ResourceLifetime::Persistent,
    })?;
    let stage_type = StageTypeId::new(STAGE_GRID_2D_FLOW);
    let capabilities = CapabilitySet::new([capability]);
    registry.register_stage(StageTypeDescriptor::gpu_only(
        stage_type.clone(),
        "Grid2D Flow",
        capabilities.clone(),
    ))?;
    let module = super::fluid_module(
        MODULE_GRID_2D_FLOW,
        "Vortex",
        "Writes a two-channel rotating velocity field for Follow Field particles.",
        CapabilityExpression::AnyOf(capabilities),
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(
        grid_inputs()
            .into_iter()
            .chain([InputMetadata::new(
                "speed",
                "Flow Speed",
                "Peak velocity scale of the vortex. A negative value reverses its direction.",
                Value::Scalar(42.0),
                aestra_extension::InputControl::Number {
                    step: 1.0,
                    min: None,
                    max: None,
                },
            )
            .with_unit("units/s")])
            .collect(),
    );
    registry.register_module(module)?;
    registry.lowering.register_module(
        ModuleTypeId::new(MODULE_GRID_2D_FLOW),
        Arc::new(GridModule(FLOW_ENTRY)),
    )?;
    registry
        .lowering
        .register_stage(stage_type, Arc::new(FlowStage))?;
    registry.register_program(ComputeProgram {
        id: ComputeProgramId::new(FLOW_PROGRAM),
        wgsl: include_str!("grid_2d_flow.wgsl").into(),
        entry_points: vec![FLOW_ENTRY.into()],
    })
}

/// A reference Grid2D effect with one diffusion stage and no particle emitters.
pub fn grid_2d_diffusion_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = grid_effect("Grid2D Diffusion", 4.0);
    effect.simulation_stages.push(grid_stage(
        registry,
        "Grid2D",
        STAGE_GRID_2D_DIFFUSION,
        MODULE_GRID_2D_DIFFUSION,
    ));
    effect.extensions = registry.derive_requirements(&effect);
    effect
}

/// A reference Grid2D effect whose particle emitter follows its XY velocity field.
pub fn grid_2d_flow_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = grid_effect("Grid2D Flow", 8.0);
    let stage = grid_stage(
        registry,
        "Grid2D Flow",
        STAGE_GRID_2D_FLOW,
        MODULE_GRID_2D_FLOW,
    );
    let mut emitter = Emitter::basic_sprite("Flow Tracers", effect.duration);
    emitter.max_particles = 512;
    emitter.transform.translation = [15.0, 0.0, 0.0];
    let mut follow = ModuleInstance::follow_field(24.0);
    follow.parameters = ModuleParameters::FollowField {
        domain: stage.name.clone(),
        field: RESOURCE_GRID_2D_VELOCITY.into(),
        strength: 24.0,
    };
    emitter.modules = vec![
        ModuleInstance::emission(85.0, 12),
        ModuleInstance::shape(EmitterShape::Sphere { radius: 2.5 }),
        ModuleInstance::initialize(
            ScalarRange::new(2.5, 4.0),
            ScalarRange::new(0.0, 0.0),
            [0.0, 1.0, 0.0],
            0.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0; 3], 0.0, 0.0),
        follow,
        ModuleInstance::appearance(
            aestra_core::Curve::new(vec![aestra_core::CurveKey::new(0.0, 1.7)]),
            aestra_core::Curve::new(vec![
                aestra_core::CurveKey::new(0.0, 0.85),
                aestra_core::CurveKey::new(0.7, 0.7),
                aestra_core::CurveKey::new(1.0, 0.0),
            ]),
            aestra_core::Gradient::new(vec![
                aestra_core::ColorKey::new(0.0, [0.38, 0.9, 1.0, 1.0]),
                aestra_core::ColorKey::new(1.0, [0.65, 0.25, 1.0, 1.0]),
            ]),
        ),
    ];
    effect.simulation_stages.push(stage);
    effect.emitters.push(emitter);
    effect.extensions = registry.derive_requirements(&effect);
    effect
}

fn grid_effect(name: &str, duration: f32) -> EffectAsset {
    let mut effect = EffectAsset::new(name, duration);
    effect.playback_mode = EffectPlaybackMode::LoopRestart;
    effect.metadata.insert(
        EffectDimension::METADATA_KEY.into(),
        EffectDimension::TwoD.metadata_value().into(),
    );
    effect
}

fn grid_stage(
    registry: &ExtensionRegistry,
    name: &str,
    stage_type: &str,
    module_type: &str,
) -> EffectSimulationStage {
    let mut stage = EffectSimulationStage::new(name, StageTypeId::new(stage_type));
    let mut module = registry
        .modules
        .instantiate(&ModuleTypeId::new(module_type))
        .expect("the fluid extension is installed");
    module.stage = StageKind::Simulation(stage.name.clone());
    stage.modules.push(module);
    stage
}

struct GridModule(&'static str);

impl ModuleLowerer for GridModule {
    fn lower(
        &self,
        module: &ModuleInstance,
        payload: &PropertyBag,
    ) -> Result<ExtensionModulePlan, String> {
        Ok(ExtensionModulePlan {
            source: module.id,
            module_type: module.module_type.clone(),
            entry_point: self.0.into(),
            parameters: payload.clone(),
            host_fields: Default::default(),
        })
    }
}

struct DiffusionStage;

fn resolve_grid(parameters: &PropertyBag) -> Result<(Grid2dLayout, [u32; 2]), String> {
    let width = parameters.get_u32("width").ok_or("invalid Grid2D width")?;
    let height = parameters
        .get_u32("height")
        .ok_or("invalid Grid2D height")?;
    let cell_size = parameters
        .get_f32("cell_size")
        .ok_or("invalid Grid2D cell size")?;
    let dimensions = [width, height];
    let origin = dimensions.map(|count| -(count as f32 * cell_size) * 0.5);
    let grid =
        Grid2dLayout::new(dimensions, origin, cell_size).map_err(|error| error.to_string())?;
    Ok((grid, dimensions))
}

fn resource(id: &str, bytes: u64, lifetime: ResourceLifetime) -> ResourceDescriptor {
    ResourceDescriptor {
        id: ResourceTypeId::new(id),
        bytes,
        lifetime,
    }
}

impl StageLowerer for DiffusionStage {
    fn lower(&self, input: &StageLoweringInput<'_>) -> Result<ExecutionBlock, String> {
        let module = match input.modules {
            [] => return Ok(ExecutionBlock::default()),
            [module] if module.module_type.0 == MODULE_GRID_2D_DIFFUSION => module,
            _ => return Err("Grid2D Diffusion expects one Diffusion module".into()),
        };
        let (grid, [width, height]) = resolve_grid(&module.parameters)?;
        let rate = module
            .parameters
            .get_f32("rate")
            .filter(|rate| (0.0..=0.25).contains(rate))
            .ok_or("diffusion rate must be between 0 and 0.25")?;
        let field = grid.field(
            ResourceTypeId::new(RESOURCE_GRID_2D_DENSITY),
            Grid2dChannels::Scalar,
        );
        let bytes = field.cells() * field.cell_bytes();
        let constants = vec![width, height, rate.to_bits()];
        Ok(ExecutionBlock {
            resources: vec![
                resource(
                    RESOURCE_GRID_2D_DENSITY,
                    bytes,
                    ResourceLifetime::Persistent,
                ),
                resource(RESOURCE_GRID_2D_NEXT, bytes, ResourceLifetime::Transient),
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
                    name: "Grid2D Diffusion".into(),
                    program: Some(ComputeProgramId::new(DIFFUSION_PROGRAM)),
                    entry_point: DIFFUSION_ENTRY.into(),
                    accesses: vec![
                        ResourceAccess::read(RESOURCE_GRID_2D_DENSITY),
                        ResourceAccess::write(RESOURCE_GRID_2D_NEXT),
                        ResourceAccess::read(AESTRA_RESOURCE_STAGE_CONSTANTS),
                        ResourceAccess::read(AESTRA_RESOURCE_FRAME),
                    ],
                    dispatch: grid.dispatch([WORKGROUP; 2]),
                    indirect: None,
                }),
                ExecutionOp::Copy(CopyOp {
                    from: ResourceTypeId::new(RESOURCE_GRID_2D_NEXT),
                    to: field.resource.clone(),
                }),
            ],
            constants,
            fields: vec![field],
            ..Default::default()
        })
    }
}

struct FlowStage;

impl StageLowerer for FlowStage {
    fn lower(&self, input: &StageLoweringInput<'_>) -> Result<ExecutionBlock, String> {
        let module = match input.modules {
            [] => return Ok(ExecutionBlock::default()),
            [module] if module.module_type.0 == MODULE_GRID_2D_FLOW => module,
            _ => return Err("Grid2D Flow expects one Vortex module".into()),
        };
        let (grid, [width, height]) = resolve_grid(&module.parameters)?;
        let speed = module
            .parameters
            .get_f32("speed")
            .filter(|speed| speed.is_finite())
            .ok_or("invalid Grid2D flow speed")?;
        let velocity = grid.field(
            ResourceTypeId::new(RESOURCE_GRID_2D_VELOCITY),
            Grid2dChannels::Vector2,
        );
        let constants = vec![width, height, speed.to_bits()];
        Ok(ExecutionBlock {
            resources: vec![
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
            ops: vec![ExecutionOp::Compute(ComputeOp {
                name: "Grid2D Vortex".into(),
                program: Some(ComputeProgramId::new(FLOW_PROGRAM)),
                entry_point: FLOW_ENTRY.into(),
                accesses: vec![
                    ResourceAccess::write(RESOURCE_GRID_2D_VELOCITY),
                    ResourceAccess::read(AESTRA_RESOURCE_STAGE_CONSTANTS),
                    ResourceAccess::read(AESTRA_RESOURCE_FRAME),
                ],
                dispatch: grid.dispatch([WORKGROUP; 2]),
                indirect: None,
            })],
            constants,
            fields: vec![velocity],
            ..Default::default()
        })
    }
}
