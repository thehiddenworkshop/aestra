//! Aestra Fluid — an experimental, GPU-only fluid extension (extensible-stages M13).
//!
//! Fluids are the architecture's stress test, not core functionality: this crate depends only on the
//! public SDK (`aestra-extension`), the Execution IR (`aestra-runtime`) and the host-binding GPU ABI
//! (`aestra-gpu`), and adds nothing to core — no `Fluid` enum variant, no fluid branch in any panel.
//! Under its own `org.example.aestra-fluid::` namespace it registers:
//!
//! - a **stage type**, *Fluid Solver*, declared GPU-only (`cpu_reference = Unavailable`, plan §13.3 /
//!   §44.6): there is no CPU fluid and none is faked;
//! - a **domain** (`grid3d`) and its **resources** — persistent velocity and density grids, transient
//!   scratch for advection, pressure, divergence and vorticity;
//! - four solver **modules**, edited by the schema-driven inspector like any built-in: *Fluid Grid*
//!   (one per stage: resolution, cell size, pressure iterations, dissipation), *Density Source* (whose
//!   position and velocity a host object can drive), *Buoyancy* and *Vorticity*;
//! - *Turbulence*: an animated noise push that breaks the regularity of a steady source;
//! - *Combustion* (fluid F3): sources also emit fuel and heat; fuel burns above an ignition
//!   temperature into heat and smoke, hot gas rises and cools. It adds the temperature and fuel grids
//!   (declared last, only with this module, so a smoke-only stage allocates nothing for fire);
//! - one presentation module, *Volume Look* (fluid F3): the stage lowerer turns it into a volume
//!   presentation — the density ray-marched as lit, self-shadowed smoke, and the temperature, when the
//!   stage burns, as blackbody fire — never into the solver, so a look edit never restarts the
//!   simulation;
//! - two **programs**: the solver (`solver.wgsl`) whose entry points the stage lowers to, and the
//!   volume look's march function (`volume.wgsl`).
//!
//! The authored stage stays one semantic object; lowering expands it into the solver's passes:
//!
//! ```text
//! add_sources (density/velocity injection, buoyancy)
//! [add_heat → combust]                             with a Combustion module
//! apply_buoyancy (density and, with fire, temperature lift the y faces; a Turbulence module's push)
//! [compute_vorticity → vorticity_force → apply_vorticity] with a Vorticity module
//! advect_velocity → copy
//! compute_divergence → repeat ×N { relax_pressure → copy } → project
//! advect_density → copy
//! [advect_temperature → copy → advect_fuel → copy] with a Combustion module
//! ```
//!
//! Every pass is a gather (no atomics), so same asset + seed + frames reproduces the same bits on the
//! same GPU. Call [`link`] once at startup.

use aestra_core::{
    CapabilityId, ComputeProgramId, DomainTypeId, EffectAsset, EffectSimulationStage, Emitter,
    ExtensionId, ModuleInstance, ModuleTypeId, PropertyBag, ResourceTypeId, StageKind, StageTypeId,
    Value, ValueType,
};
use aestra_extension::{
    AestraExtension, CapabilityExpression, CapabilitySet, ComputeProgram, DomainDescriptor,
    ExtensionManifest, ExtensionRegistry, InputControl, InputMetadata, InputSourceKind,
    ModuleLowerer, ModuleMetadata, ModuleMultiplicity, NeighborhoodRequirement, RegistryConflict,
    ResourceTypeDescriptor, SimulationRequirements, StageLowerer, StageLoweringInput,
    StageTypeDescriptor, SynchronizationRequirement, TemporalRequirement,
};
use aestra_runtime::{
    AESTRA_RESOURCE_FRAME, AESTRA_RESOURCE_HOST_BINDINGS, AESTRA_RESOURCE_STAGE_CONSTANTS,
    CompiledHostFieldRef, ComputeOp, CopyOp, ExecutionBlock, ExecutionOp, ExtensionModulePlan,
    FieldLayout, RepeatPolicy, ResourceAccess, ResourceDescriptor, ResourceLifetime,
    StagePresentation, StagedDispatch, VolumePresentation,
};
use std::sync::Arc;

pub const PLUGIN_ID: &str = "org.example.aestra-fluid";
pub const CAPABILITY_FLUID_GRID: &str = "org.example.aestra-fluid::capability/fluid_grid";
pub const DOMAIN_GRID3D: &str = "org.example.aestra-fluid::domain/grid3d";
pub const STAGE_FLUID_SOLVER: &str = "org.example.aestra-fluid::stage/fluid_solver";
pub const MODULE_GRID: &str = "org.example.aestra-fluid::module/grid";
pub const MODULE_DENSITY_SOURCE: &str = "org.example.aestra-fluid::module/density_source";
pub const MODULE_BUOYANCY: &str = "org.example.aestra-fluid::module/buoyancy";
pub const MODULE_VORTICITY: &str = "org.example.aestra-fluid::module/vorticity";
pub const MODULE_TURBULENCE: &str = "org.example.aestra-fluid::module/turbulence";
pub const MODULE_VOLUME_LOOK: &str = "org.example.aestra-fluid::module/volume_look";
pub const MODULE_COMBUSTION: &str = "org.example.aestra-fluid::module/combustion";
pub const MODULE_SPHERE_COLLIDER: &str = "org.example.aestra-fluid::module/sphere_collider";
pub const MODULE_BOX_COLLIDER: &str = "org.example.aestra-fluid::module/box_collider";
pub const MODULE_CAPSULE_COLLIDER: &str = "org.example.aestra-fluid::module/capsule_collider";
pub const PROGRAM_SOLVER: &str = "org.example.aestra-fluid::program/solver";
pub const PROGRAM_VOLUME: &str = "org.example.aestra-fluid::program/volume";
/// The march function [`PROGRAM_VOLUME`] defines.
pub const VOLUME_ENTRY: &str = "fluid_volume";

pub const RESOURCE_VELOCITY: &str = "org.example.aestra-fluid::resource/velocity_grid";
pub const RESOURCE_DENSITY: &str = "org.example.aestra-fluid::resource/density_grid";
pub const RESOURCE_VELOCITY_NEXT: &str = "org.example.aestra-fluid::resource/velocity_scratch";
pub const RESOURCE_DENSITY_NEXT: &str = "org.example.aestra-fluid::resource/density_scratch";
pub const RESOURCE_PRESSURE: &str = "org.example.aestra-fluid::resource/pressure_grid";
pub const RESOURCE_PRESSURE_NEXT: &str = "org.example.aestra-fluid::resource/pressure_scratch";
pub const RESOURCE_DIVERGENCE: &str = "org.example.aestra-fluid::resource/divergence_grid";
pub const RESOURCE_VORTICITY: &str = "org.example.aestra-fluid::resource/vorticity_grid";
pub const RESOURCE_VELOCITY_HAT: &str = "org.example.aestra-fluid::resource/velocity_corrected";
pub const RESOURCE_SCALAR_HAT: &str = "org.example.aestra-fluid::resource/scalar_corrected";
pub const RESOURCE_SOLID: &str = "org.example.aestra-fluid::resource/solid_grid";
pub const RESOURCE_TEMPERATURE: &str = "org.example.aestra-fluid::resource/temperature_grid";
pub const RESOURCE_TEMPERATURE_NEXT: &str =
    "org.example.aestra-fluid::resource/temperature_scratch";
pub const RESOURCE_FUEL: &str = "org.example.aestra-fluid::resource/fuel_grid";
pub const RESOURCE_FUEL_NEXT: &str = "org.example.aestra-fluid::resource/fuel_scratch";

/// The solver's WGSL; see [`program_wgsl`] for the full program with the host-binding accessors.
pub const SOLVER_WGSL: &str = include_str!("solver.wgsl");
/// The volume look's march function, composed after the backend's volume interface.
pub const VOLUME_WGSL: &str = include_str!("volume.wgsl");

/// The grid resolution per axis is bounded (plan §11.1): at 96³ every grid together is ~60 MB.
pub const MIN_RESOLUTION: u32 = 8;
pub const MAX_RESOLUTION: u32 = 96;
pub const MAX_PRESSURE_ITERATIONS: u32 = 200;
/// Density sources one stage packs into its constants.
pub const MAX_SOURCES: usize = 8;
/// The solver's workgroup edge; the resolution must be a multiple of it.
pub const WORKGROUP: u32 = 4;

/// The stage-constant layout `solver.wgsl` reads (words).
const SOURCE_BASE: usize = 18;
/// Words one collider record takes (see `pack_collider`).
const COLLIDER_WORDS: usize = 24;
/// Colliders one stage packs into its constants.
pub const MAX_COLLIDERS: usize = 4;
/// Open-side bits of constant word 10 (fluid F4): `1 << (2·axis + side)`.
const OPEN_X_MIN: u32 = 1 << 0;
const OPEN_X_MAX: u32 = 1 << 1;
const OPEN_Y_MAX: u32 = 1 << 3;
const OPEN_Z_MIN: u32 = 1 << 4;
const OPEN_Z_MAX: u32 = 1 << 5;
const SOURCE_WORDS: usize = 16;
const NO_SLOT: u32 = u32::MAX;

/// Every fluid module is stateful, iterative and reads a grid neighbourhood — the compiler derives
/// the staged simulation class (and restart/replay seeking) from this.
const STAGED: SimulationRequirements = SimulationRequirements {
    temporal: TemporalRequirement::PreviousState,
    synchronization: SynchronizationRequirement::Iterative,
    neighborhood: NeighborhoodRequirement::Grid,
};

/// The fluid extension. Stateless: everything it contributes is registered in `register`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluidExtension;

/// Links the fluid extension into this process, so `EffectCompiler::default()` and
/// `ExtensionRegistry::linked()` include it. Idempotent.
pub fn link() {
    aestra_extension::link_extension(Arc::new(FluidExtension))
        .expect("the fluid extension registers only namespaced, unique ids");
}

/// The complete solver program: the solver passes plus the host-binding accessors they call.
pub fn program_wgsl() -> String {
    format!("{SOLVER_WGSL}\n{}", aestra_gpu::HOST_BINDINGS_WGSL)
}

/// The simulation-stage name [`smoke_effect`] authors.
pub const SMOKE_STAGE: &str = "Fluid";

/// An effect whose own *Fluid Solver* domain (an effect-level simulation stage, fluid F2) hosts a
/// Fluid Grid, a Density Source, Buoyancy, Vorticity and a Volume Look — every input at its schema
/// default — beside a sprite emitter. `registry` must have the extension installed.
pub fn smoke_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = EffectAsset::new("Fluid Smoke", 4.0);
    let mut domain = EffectSimulationStage::new(SMOKE_STAGE, StageTypeId::new(STAGE_FLUID_SOLVER));
    for type_id in [
        MODULE_GRID,
        MODULE_DENSITY_SOURCE,
        MODULE_BUOYANCY,
        MODULE_VORTICITY,
        MODULE_VOLUME_LOOK,
    ] {
        let mut module = registry
            .modules
            .instantiate(&ModuleTypeId::new(type_id))
            .expect("the fluid extension is installed in the registry");
        module.stage = StageKind::Simulation(SMOKE_STAGE.into());
        domain.modules.push(module);
    }
    effect.simulation_stages.push(domain);
    effect.emitters.push(Emitter::basic_sprite("Smoke", 4.0));
    effect
}

/// [`smoke_effect`] set on fire (fluid F3): a Combustion module, and a source emitting fuel and the
/// heat that ignites it, with little smoke of its own — the smoke comes from burning.
pub fn fire_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = smoke_effect(registry);
    effect.name = "Fluid Fire".into();
    let domain = &mut effect.simulation_stages[0];
    let mut combustion = registry
        .modules
        .instantiate(&ModuleTypeId::new(MODULE_COMBUSTION))
        .expect("the fluid extension is installed in the registry");
    combustion.stage = StageKind::Simulation(SMOKE_STAGE.into());
    domain.modules.push(combustion);
    let source = domain
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_DENSITY_SOURCE)
        .expect("the smoke effect has a source");
    if let aestra_core::ModuleParameters::Custom(values) = &mut source.parameters {
        for (name, value) in [
            ("density_rate", 0.5),
            ("temperature_rate", 3.0),
            ("fuel_rate", 4.0),
        ] {
            values.insert(name.into(), Value::Scalar(value));
        }
    }
    effect
}

/// The solver's entry points, as its compute ops name them.
pub const ENTRY_POINTS: [&str; 20] = [
    "add_sources",
    "compute_vorticity",
    "vorticity_force",
    "apply_vorticity",
    "advect_velocity",
    "advect_density",
    "compute_divergence",
    "relax_pressure",
    "project",
    "add_heat",
    "combust",
    "advect_temperature",
    "advect_fuel",
    "apply_buoyancy",
    "apply_buoyancy_fire",
    "correct_velocity",
    "correct_density",
    "correct_temperature",
    "correct_fuel",
    "mark_solids",
];

impl AestraExtension for FluidExtension {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            plugin: ExtensionId::new(PLUGIN_ID),
            display_name: "Aestra Fluid".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    fn register(&self, registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
        let fluid_grid = CapabilityId::new(CAPABILITY_FLUID_GRID);
        registry.register_capability(fluid_grid.clone())?;
        registry.domains.register(DomainDescriptor {
            type_id: DomainTypeId::new(DOMAIN_GRID3D),
            display_name: "Fluid Grid".into(),
        })?;
        for (type_id, display_name, lifetime) in [
            (RESOURCE_VELOCITY, "Velocity", ResourceLifetime::Persistent),
            (RESOURCE_DENSITY, "Density", ResourceLifetime::Persistent),
            (
                RESOURCE_VELOCITY_NEXT,
                "Velocity Scratch",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_DENSITY_NEXT,
                "Density Scratch",
                ResourceLifetime::Transient,
            ),
            (RESOURCE_PRESSURE, "Pressure", ResourceLifetime::Transient),
            (
                RESOURCE_PRESSURE_NEXT,
                "Pressure Scratch",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_DIVERGENCE,
                "Divergence",
                ResourceLifetime::Transient,
            ),
            (RESOURCE_VORTICITY, "Vorticity", ResourceLifetime::Transient),
            (
                RESOURCE_VELOCITY_HAT,
                "Corrected Velocity",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_SCALAR_HAT,
                "Corrected Scalar",
                ResourceLifetime::Transient,
            ),
            (RESOURCE_SOLID, "Solids", ResourceLifetime::Transient),
            (
                RESOURCE_TEMPERATURE,
                "Temperature",
                ResourceLifetime::Persistent,
            ),
            (
                RESOURCE_TEMPERATURE_NEXT,
                "Temperature Scratch",
                ResourceLifetime::Transient,
            ),
            (RESOURCE_FUEL, "Fuel", ResourceLifetime::Persistent),
            (
                RESOURCE_FUEL_NEXT,
                "Fuel Scratch",
                ResourceLifetime::Transient,
            ),
        ] {
            registry.resources.register(ResourceTypeDescriptor {
                type_id: ResourceTypeId::new(type_id),
                display_name: display_name.into(),
                domain: DomainTypeId::new(DOMAIN_GRID3D),
                lifetime,
            })?;
        }
        registry.register_stage(StageTypeDescriptor::gpu_only(
            StageTypeId::new(STAGE_FLUID_SOLVER),
            "Fluid Solver",
            CapabilitySet::new([fluid_grid.clone()]),
        ))?;
        let requires = CapabilityExpression::AnyOf(CapabilitySet::new([fluid_grid]));
        for metadata in [
            grid_metadata(requires.clone()),
            density_source_metadata(requires.clone()),
            buoyancy_metadata(requires.clone()),
            vorticity_metadata(requires.clone()),
            turbulence_metadata(requires.clone()),
            combustion_metadata(requires.clone()),
            collider_metadata(MODULE_SPHERE_COLLIDER, requires.clone()),
            collider_metadata(MODULE_BOX_COLLIDER, requires.clone()),
            collider_metadata(MODULE_CAPSULE_COLLIDER, requires.clone()),
            volume_look_metadata(requires),
        ] {
            let type_id = metadata.type_id.clone();
            registry.register_module(metadata)?;
            registry
                .lowering
                .register_module(type_id, Arc::new(FluidModuleLowerer))?;
        }
        registry.register_program(ComputeProgram {
            id: ComputeProgramId::new(PROGRAM_SOLVER),
            wgsl: program_wgsl(),
            entry_points: ENTRY_POINTS.iter().map(|entry| entry.to_string()).collect(),
        })?;
        registry.register_program(ComputeProgram {
            id: ComputeProgramId::new(PROGRAM_VOLUME),
            wgsl: VOLUME_WGSL.into(),
            entry_points: vec![VOLUME_ENTRY.into()],
        })?;
        registry.lowering.register_stage(
            StageTypeId::new(STAGE_FLUID_SOLVER),
            Arc::new(FluidSolverLowerer),
        )?;
        Ok(())
    }
}

fn number(step: f32, min: f32, max: Option<f32>) -> InputControl {
    InputControl::Number {
        step,
        min: Some(min),
        max,
    }
}

fn vector() -> InputControl {
    InputControl::Vector {
        step: 0.1,
        min: None,
        max: None,
    }
}

fn fluid_module(
    type_id: &str,
    display_name: &'static str,
    description: &'static str,
    requires: CapabilityExpression,
) -> ModuleMetadata {
    ModuleMetadata::extension(
        ModuleTypeId::new(type_id),
        display_name,
        description,
        "Fluid",
        requires,
    )
    .with_simulation(STAGED)
    .with_tags(vec!["plugin", "fluid"])
}

fn grid_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    fluid_module(
        MODULE_GRID,
        "Fluid Grid",
        "The solver's grid: its size, placement, pressure iterations and dissipation.",
        requires,
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![
        InputMetadata::new(
            "resolution",
            "Resolution",
            "Cells per axis (a multiple of 4).",
            Value::U32(32),
            number(
                WORKGROUP as f32,
                MIN_RESOLUTION as f32,
                Some(MAX_RESOLUTION as f32),
            ),
        ),
        InputMetadata::new(
            "cell_size",
            "Cell Size",
            "Edge length of one grid cell.",
            Value::Scalar(3.0),
            number(0.1, 0.001, None),
        )
        .with_unit("units"),
        InputMetadata::new(
            "center",
            "Center",
            "Where the grid's centre sits.",
            Value::Vec3([0.0, 48.0, 0.0]),
            vector(),
        )
        .with_unit("units"),
        InputMetadata::new(
            "pressure_iterations",
            "Pressure Iterations",
            "Jacobi iterations of the pressure solve per tick.",
            Value::U32(24),
            number(1.0, 1.0, Some(MAX_PRESSURE_ITERATIONS as f32)),
        ),
        InputMetadata::new(
            "density_dissipation",
            "Density Dissipation",
            "How fast density fades, per second.",
            Value::Scalar(0.1),
            number(0.01, 0.0, None),
        ),
        InputMetadata::new(
            "velocity_dissipation",
            "Velocity Dissipation",
            "How fast motion fades, per second. Sources, buoyancy and heat keep adding motion, and \
             sharp advection keeps almost all of it: too little dissipation lets it build up until \
             the whole grid churns and the smoke fills it.",
            Value::Scalar(0.5),
            number(0.01, 0.0, None),
        ),
        InputMetadata::new(
            "open_top",
            "Open Top",
            "Fluid leaves through the top of the grid instead of pooling under it.",
            Value::Bool(true),
            InputControl::Toggle,
        ),
        InputMetadata::new(
            "open_sides",
            "Open Sides",
            "Fluid leaves through the four sides of the grid (the floor stays closed).",
            Value::Bool(false),
            InputControl::Toggle,
        ),
        InputMetadata::new(
            "sharp_advection",
            "Sharp Advection",
            "MacCormack advection: keeps swirls and edges that plain semi-Lagrangian advection blurs, for two extra passes per field.",
            Value::Bool(true),
            InputControl::Toggle,
        ),
    ])
    .with_cost(8)
}

fn density_source_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    let bindable = vec![InputSourceKind::Constant, InputSourceKind::HostBinding];
    fluid_module(
        MODULE_DENSITY_SOURCE,
        "Density Source",
        "Injects density and pushes the fluid inside a sphere.",
        requires,
    )
    .with_inputs(vec![
        InputMetadata::new(
            "position",
            "Position",
            "Centre of the source; a host object can drive it.",
            Value::Vec3([0.0, 15.0, 0.0]),
            vector(),
        )
        .with_unit("units")
        .with_sources(bindable.clone())
        .with_position_handle(),
        InputMetadata::new(
            "radius",
            "Radius",
            "Radius of the source sphere.",
            Value::Scalar(12.0),
            number(0.5, 0.01, None),
        )
        .with_unit("units"),
        InputMetadata::new(
            "density_rate",
            "Density Rate",
            "Density added per second at the centre.",
            Value::Scalar(5.0),
            number(0.1, 0.0, None),
        ),
        InputMetadata::new(
            "velocity",
            "Velocity",
            "Velocity the source imparts; a host object's motion can drive it.",
            Value::Vec3([0.0, 40.0, 0.0]),
            vector(),
        )
        .with_unit("units/s")
        .with_sources(bindable),
        InputMetadata::new(
            "temperature_rate",
            "Temperature Rate",
            "Heat added per second at the centre (needs a Combustion module).",
            Value::Scalar(0.0),
            number(0.1, 0.0, None),
        ),
        InputMetadata::new(
            "fuel_rate",
            "Fuel Rate",
            "Fuel added per second at the centre (needs a Combustion module).",
            Value::Scalar(0.0),
            number(0.1, 0.0, None),
        ),
    ])
    .with_cost(2)
}

/// Fire (fluid F3): fuel burns where the temperature passes the ignition point, releasing heat and
/// smoke; hot gas rises and cools. Adds the temperature and fuel grids.
fn combustion_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    let input = |name, display, description, value: f32, step| {
        InputMetadata::new(
            name,
            display,
            description,
            Value::Scalar(value),
            number(step, 0.0, None),
        )
    };
    fluid_module(
        MODULE_COMBUSTION,
        "Combustion",
        "Burns fuel into heat and smoke: fire. Sources emit the fuel and the heat that ignites it.",
        requires,
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![
        input(
            "ignition_temperature",
            "Ignition Temperature",
            "Temperature above which fuel burns.",
            0.5,
            0.05,
        ),
        input(
            "burn_rate",
            "Burn Rate",
            "Fuel burned per second where it is hot enough.",
            2.0,
            0.1,
        ),
        input(
            "heat_release",
            "Heat Release",
            "Temperature gained per unit of fuel burned.",
            3.0,
            0.1,
        ),
        input(
            "smoke_yield",
            "Smoke Yield",
            "Density produced per unit of fuel burned.",
            0.6,
            0.05,
        ),
        input(
            "cooling",
            "Cooling",
            "How fast temperature fades, per second.",
            1.0,
            0.05,
        ),
        input(
            "thermal_lift",
            "Thermal Lift",
            "Upward acceleration per unit of temperature.",
            30.0,
            1.0,
        ),
    ])
    .with_cost(3)
}

/// An analytic collider (fluid F4): the fluid flows around it — no flow through it, and a moving one
/// pushes the fluid. Its centre and velocity can follow a host object; the centre has a viewport
/// handle. A sphere has a radius, a box half extents (axis-aligned in the effect's space), a capsule a
/// half-segment vector and a radius.
fn collider_metadata(type_id: &'static str, requires: CapabilityExpression) -> ModuleMetadata {
    let bindable = vec![InputSourceKind::Constant, InputSourceKind::HostBinding];
    let (display_name, description) = match type_id {
        MODULE_SPHERE_COLLIDER => (
            "Sphere Collider",
            "A sphere the fluid flows around; a host object can move it.",
        ),
        MODULE_BOX_COLLIDER => (
            "Box Collider",
            "An axis-aligned box the fluid flows around; a host object can move it.",
        ),
        _ => (
            "Capsule Collider",
            "A capsule (a swept segment) the fluid flows around; a host object can move it.",
        ),
    };
    let mut inputs = vec![
        InputMetadata::new(
            "position",
            "Position",
            "Centre of the collider; a host object can drive it.",
            Value::Vec3([0.0, 40.0, 0.0]),
            vector(),
        )
        .with_unit("units")
        .with_sources(bindable.clone())
        .with_position_handle(),
        InputMetadata::new(
            "velocity",
            "Velocity",
            "How fast the collider moves, which it imparts to the fluid; a host object's motion can drive it.",
            Value::Vec3([0.0; 3]),
            vector(),
        )
        .with_unit("units/s")
        .with_sources(bindable),
    ];
    match type_id {
        MODULE_BOX_COLLIDER => inputs.push(
            InputMetadata::new(
                "half_extents",
                "Half Extents",
                "Half the box's size along each axis.",
                Value::Vec3([8.0, 8.0, 8.0]),
                vector(),
            )
            .with_unit("units"),
        ),
        MODULE_CAPSULE_COLLIDER => inputs.push(
            InputMetadata::new(
                "half_segment",
                "Half Segment",
                "From the centre to one end of the capsule's axis.",
                Value::Vec3([0.0, 10.0, 0.0]),
                vector(),
            )
            .with_unit("units"),
        ),
        _ => {}
    }
    if type_id != MODULE_BOX_COLLIDER {
        inputs.push(
            InputMetadata::new(
                "radius",
                "Radius",
                "Radius of the sphere, or of the capsule around its axis.",
                Value::Scalar(8.0),
                number(0.5, 0.01, None),
            )
            .with_unit("units"),
        );
    }
    inputs.push(InputMetadata::new(
        "no_slip",
        "Sticky",
        "The fluid sticks to the surface (no slip) instead of sliding along it.",
        Value::Bool(false),
        InputControl::Toggle,
    ));
    fluid_module(type_id, display_name, description, requires)
        .with_inputs(inputs)
        .with_cost(2)
}

/// One collider's constant record (`COLLIDER_WORDS`), as `collider_distance` and `mark_solids` read
/// it: kind, centre (value + host reference), velocity (value + host reference), size, radius, sticky.
fn pack_collider(collider: &ExtensionModulePlan) -> Result<[u32; COLLIDER_WORDS], String> {
    let parameters = &collider.parameters;
    let mut words = [0u32; COLLIDER_WORDS];
    let (kind, size, radius) = match collider.module_type.0.as_str() {
        MODULE_SPHERE_COLLIDER => (0, [0.0; 3], scalar(parameters, "radius")?),
        MODULE_BOX_COLLIDER => (1, vec3(parameters, "half_extents")?, 0.0),
        _ => (
            2,
            vec3(parameters, "half_segment")?,
            scalar(parameters, "radius")?,
        ),
    };
    if radius < 0.0 || size.iter().any(|axis| *axis < 0.0 && kind == 1) {
        return Err("collider sizes must not be negative".into());
    }
    if kind != 1 && radius <= 0.0 {
        return Err("a collider's radius must be positive".into());
    }
    words[0] = kind;
    let position = vec3(parameters, "position")?;
    let velocity = vec3(parameters, "velocity")?;
    for axis in 0..3 {
        words[1 + axis] = position[axis].to_bits();
        words[7 + axis] = velocity[axis].to_bits();
        words[13 + axis] = size[axis].to_bits();
    }
    words[4..7].copy_from_slice(&host_ref(collider.host_fields.get("position"))?);
    words[10..13].copy_from_slice(&host_ref(collider.host_fields.get("velocity"))?);
    words[16] = radius.to_bits();
    words[17] = u32::from(parameters.get_bool("no_slip").unwrap_or(false));
    Ok(words)
}

fn is_collider(module_type: &str) -> bool {
    matches!(
        module_type,
        MODULE_SPHERE_COLLIDER | MODULE_BOX_COLLIDER | MODULE_CAPSULE_COLLIDER
    )
}

/// The Combustion inputs, in the order `solver.wgsl` reads them after the sources.
const COMBUSTION_INPUTS: [&str; 6] = [
    "ignition_temperature",
    "burn_rate",
    "heat_release",
    "smoke_yield",
    "cooling",
    "thermal_lift",
];

fn buoyancy_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    fluid_module(
        MODULE_BUOYANCY,
        "Buoyancy",
        "Lifts dense (hot) fluid upward.",
        requires,
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![InputMetadata::new(
        "strength",
        "Strength",
        "Upward acceleration per unit density.",
        Value::Scalar(40.0),
        number(1.0, 0.0, None),
    )])
    .with_cost(1)
}

fn vorticity_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    fluid_module(
        MODULE_VORTICITY,
        "Vorticity",
        "Vorticity confinement: restores the small swirls numerical diffusion smooths away.",
        requires,
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![InputMetadata::new(
        "strength",
        "Strength",
        "Confinement strength.",
        Value::Scalar(0.4),
        number(0.05, 0.0, None),
    )])
    .with_cost(2)
}

fn turbulence_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    fluid_module(
        MODULE_TURBULENCE,
        "Turbulence",
        "Stirs the fluid with an ever-changing swirling push, so plumes and flames waver and break \
         up like real ones instead of rising in a perfectly regular column.",
        requires,
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![
        InputMetadata::new(
            "strength",
            "Strength",
            "Acceleration of the push.",
            Value::Scalar(20.0),
            number(1.0, 0.0, None),
        ),
        InputMetadata::new(
            "scale",
            "Scale",
            "Size of the largest swirls, in the effect's units.",
            Value::Scalar(20.0),
            number(0.5, 0.01, None),
        ),
        InputMetadata::new(
            "evolution",
            "Evolution",
            "How many times per second the swirls renew (0 keeps them still).",
            Value::Scalar(1.5),
            number(0.05, 0.0, None),
        ),
        InputMetadata::new(
            "masked",
            "Only In Smoke",
            "Push only where there is smoke or heat, leaving the still air around it alone.",
            Value::Bool(true),
            InputControl::Toggle,
        ),
    ])
    .with_cost(1)
}

fn colour() -> InputControl {
    InputControl::Vector {
        step: 0.01,
        min: Some(0.0),
        max: Some(1.0),
    }
}

/// The domain's look (fluid F3). A presentation module: it lowers into the stage's volume
/// presentation, never into the solver, so editing it never restarts the simulation.
fn volume_look_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    ModuleMetadata::extension(
        ModuleTypeId::new(MODULE_VOLUME_LOOK),
        "Volume Look",
        "Draws the domain as lit smoke: the density ray-marched with self-shadowing.",
        "Fluid",
        requires,
    )
    .with_tags(vec!["plugin", "fluid", "render"])
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![
        InputMetadata::new(
            "opacity",
            "Opacity",
            "Extinction per unit of density per unit of length.",
            Value::Scalar(0.06),
            number(0.01, 0.0, None),
        ),
        InputMetadata::new(
            "color",
            "Smoke Color",
            "How much light the smoke scatters, per channel.",
            Value::Vec3([0.72, 0.74, 0.78]),
            colour(),
        ),
        InputMetadata::new(
            "ambient",
            "Ambient",
            "Light reaching the smoke from every direction.",
            Value::Scalar(0.3),
            number(0.01, 0.0, None),
        ),
        InputMetadata::new(
            "light_direction",
            "Light Direction",
            "Direction towards the light, in the effect's space.",
            Value::Vec3([-0.55, 0.75, -0.35]),
            vector(),
        ),
        InputMetadata::new(
            "light_color",
            "Light Color",
            "Colour of the light.",
            Value::Vec3([1.0, 0.96, 0.9]),
            colour(),
        ),
        InputMetadata::new(
            "light_intensity",
            "Light Intensity",
            "Brightness of the light.",
            Value::Scalar(1.2),
            number(0.05, 0.0, None),
        ),
        InputMetadata::new(
            "steps",
            "March Steps",
            "Samples along each view ray.",
            Value::U32(48),
            number(1.0, 4.0, Some(MAX_VOLUME_STEPS as f32)),
        ),
        InputMetadata::new(
            "shadow_steps",
            "Shadow Steps",
            "Samples towards the light per view sample (0 disables self-shadowing).",
            Value::U32(6),
            number(1.0, 0.0, Some(MAX_SHADOW_STEPS as f32)),
        ),
        InputMetadata::new(
            "fire_intensity",
            "Fire Intensity",
            "Brightness of burning gas (with a Combustion module).",
            Value::Scalar(1.0),
            number(0.05, 0.0, None),
        ),
        InputMetadata::new(
            "temperature_scale",
            "Temperature Scale",
            "Kelvin per unit of temperature: sets the blackbody colour of the flames.",
            Value::Scalar(1000.0),
            number(10.0, 0.0, None),
        )
        .with_unit("K"),
        InputMetadata::new(
            "edge_fade",
            "Open Edge Fade",
            "How far in from an open side of the grid smoke and flames fade out, as a fraction of the \
             grid, so they thin away instead of ending in a flat cut where they leave it.",
            Value::Scalar(0.15),
            number(0.01, 0.0, Some(0.5)),
        ),
    ])
    .with_cost(4)
}

/// Bounds of the volume look's sample counts.
pub const MAX_VOLUME_STEPS: u32 = 256;
pub const MAX_SHADOW_STEPS: u32 = 32;

fn scalar(payload: &PropertyBag, name: &str) -> Result<f32, String> {
    payload
        .get_f32(name)
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("'{name}' must be a finite number"))
}

fn vec3(payload: &PropertyBag, name: &str) -> Result<[f32; 3], String> {
    payload
        .get_vec3(name)
        .filter(|value| value.iter().all(|component| component.is_finite()))
        .ok_or_else(|| format!("'{name}' must be a finite vector"))
}

fn count(payload: &PropertyBag, name: &str) -> Result<u32, String> {
    payload
        .get_u32(name)
        .ok_or_else(|| format!("'{name}' must be a whole number"))
}

/// Validates one fluid module's resolved inputs and names the pass it drives.
struct FluidModuleLowerer;

impl ModuleLowerer for FluidModuleLowerer {
    fn lower(
        &self,
        module: &ModuleInstance,
        payload: &PropertyBag,
    ) -> Result<ExtensionModulePlan, String> {
        let entry_point = match module.module_type.0.as_str() {
            MODULE_GRID => {
                let resolution = count(payload, "resolution")?;
                if !(MIN_RESOLUTION..=MAX_RESOLUTION).contains(&resolution)
                    || resolution % WORKGROUP != 0
                {
                    return Err(format!(
                        "grid resolution must be a multiple of {WORKGROUP} between \
                         {MIN_RESOLUTION} and {MAX_RESOLUTION}, got {resolution}"
                    ));
                }
                if scalar(payload, "cell_size")? <= 0.0 {
                    return Err("grid cell size must be positive".into());
                }
                vec3(payload, "center")?;
                let iterations = count(payload, "pressure_iterations")?;
                if !(1..=MAX_PRESSURE_ITERATIONS).contains(&iterations) {
                    return Err(format!(
                        "pressure iterations must be between 1 and {MAX_PRESSURE_ITERATIONS}, \
                         got {iterations}"
                    ));
                }
                for name in ["density_dissipation", "velocity_dissipation"] {
                    if scalar(payload, name)? < 0.0 {
                        return Err(format!("'{name}' must not be negative"));
                    }
                }
                "relax_pressure"
            }
            MODULE_DENSITY_SOURCE => {
                vec3(payload, "position")?;
                vec3(payload, "velocity")?;
                if scalar(payload, "radius")? <= 0.0 {
                    return Err("source radius must be positive".into());
                }
                for name in ["density_rate", "temperature_rate", "fuel_rate"] {
                    scalar(payload, name)?;
                }
                "add_sources"
            }
            collider if is_collider(collider) => {
                pack_collider(&ExtensionModulePlan {
                    source: module.id,
                    module_type: module.module_type.clone(),
                    entry_point: String::new(),
                    parameters: payload.clone(),
                    host_fields: Default::default(),
                })?;
                "mark_solids"
            }
            MODULE_COMBUSTION => {
                for name in COMBUSTION_INPUTS {
                    if scalar(payload, name)? < 0.0 {
                        return Err(format!("'{name}' must not be negative"));
                    }
                }
                "combust"
            }
            MODULE_BUOYANCY => {
                scalar(payload, "strength")?;
                "add_sources"
            }
            MODULE_VORTICITY => {
                scalar(payload, "strength")?;
                "apply_vorticity"
            }
            MODULE_TURBULENCE => {
                if scalar(payload, "strength")? < 0.0 || scalar(payload, "evolution")? < 0.0 {
                    return Err("turbulence strength and evolution must not be negative".into());
                }
                if scalar(payload, "scale")? <= 0.0 {
                    return Err("turbulence scale must be positive".into());
                }
                "apply_buoyancy"
            }
            MODULE_VOLUME_LOOK => {
                pack_volume(payload)?;
                VOLUME_ENTRY
            }
            other => return Err(format!("'{other}' is not a fluid module")),
        };
        Ok(ExtensionModulePlan {
            source: module.id,
            module_type: module.module_type.clone(),
            entry_point: entry_point.into(),
            parameters: payload.clone(),
            // Filled by the compiler from the module's host-bound inputs (host bindings HB4).
            host_fields: Default::default(),
        })
    }
}

/// The resources a fluid stage declares, in the binding order `solver.wgsl` expects; the fire grids
/// only with combustion, last, so the others keep their bindings.
fn resources(resolution: u32, constant_words: usize, fire: bool) -> Vec<ResourceDescriptor> {
    let cells = u64::from(resolution).pow(3);
    let grid = |id: &str, bytes_per_cell: u64, lifetime| ResourceDescriptor {
        id: ResourceTypeId::new(id),
        bytes: cells * bytes_per_cell,
        lifetime,
    };
    let mut resources = vec![
        grid(RESOURCE_VELOCITY, 16, ResourceLifetime::Persistent),
        grid(RESOURCE_DENSITY, 4, ResourceLifetime::Persistent),
        grid(RESOURCE_VELOCITY_NEXT, 16, ResourceLifetime::Transient),
        grid(RESOURCE_DENSITY_NEXT, 4, ResourceLifetime::Transient),
        grid(RESOURCE_PRESSURE, 4, ResourceLifetime::Transient),
        grid(RESOURCE_PRESSURE_NEXT, 4, ResourceLifetime::Transient),
        grid(RESOURCE_DIVERGENCE, 4, ResourceLifetime::Transient),
        grid(RESOURCE_VORTICITY, 16, ResourceLifetime::Transient),
    ];
    resources.push(ResourceDescriptor {
        id: ResourceTypeId::new(AESTRA_RESOURCE_STAGE_CONSTANTS),
        bytes: constant_words as u64 * 4,
        lifetime: ResourceLifetime::Persistent,
    });
    resources.push(ExecutionBlock::frame_resource());
    // Sized by the host from the compiled effect's binding layouts (host bindings HB6).
    resources.push(ResourceDescriptor {
        id: ResourceTypeId::new(AESTRA_RESOURCE_HOST_BINDINGS),
        bytes: 0,
        lifetime: ResourceLifetime::Persistent,
    });
    // MacCormack's scratch (fluid F4): always declared, so the fire grids keep their bindings.
    resources.push(grid(RESOURCE_VELOCITY_HAT, 16, ResourceLifetime::Transient));
    resources.push(grid(RESOURCE_SCALAR_HAT, 4, ResourceLifetime::Transient));
    resources.push(grid(RESOURCE_SOLID, 16, ResourceLifetime::Transient));
    if fire {
        resources.extend([
            grid(RESOURCE_TEMPERATURE, 4, ResourceLifetime::Persistent),
            grid(RESOURCE_TEMPERATURE_NEXT, 4, ResourceLifetime::Transient),
            grid(RESOURCE_FUEL, 4, ResourceLifetime::Persistent),
            grid(RESOURCE_FUEL_NEXT, 4, ResourceLifetime::Transient),
        ]);
    }
    resources
}

/// A host-field reference as the solver reads it: `[slot, presence bit, value offset]`.
fn host_ref(source: Option<&CompiledHostFieldRef>) -> Result<[u32; 3], String> {
    match source {
        None => Ok([NO_SLOT, 0, 0]),
        Some(source) if source.value_type == ValueType::Vec3 => {
            Ok([source.binding.0 as u32, source.field_index, source.offset])
        }
        Some(source) => Err(format!(
            "host field '{}' is {:?}; a source input needs a Vec3",
            source.field.as_str(),
            source.value_type
        )),
    }
}

fn modules_of<'a>(
    modules: &'a [ExtensionModulePlan],
    type_id: &'a str,
) -> impl Iterator<Item = &'a ExtensionModulePlan> {
    modules
        .iter()
        .filter(move |module| module.module_type.0 == type_id)
}

/// A Fluid Grid's open sides (fluid F4): bit 2·axis for the minimum side, 2·axis + 1 for the maximum.
fn open_side_mask(grid: &PropertyBag) -> u32 {
    let open = |name: &str| grid.get_bool(name).unwrap_or(false);
    let mut sides = 0u32;
    if open("open_top") {
        sides |= OPEN_Y_MAX;
    }
    if open("open_sides") {
        sides |= OPEN_X_MIN | OPEN_X_MAX | OPEN_Z_MIN | OPEN_Z_MAX;
    }
    sides
}

/// A stage's packed constants plus the grid placement its field layouts declare.
struct PackedStage {
    resolution: u32,
    iterations: u32,
    cell_size: f32,
    origin: [f32; 3],
    constants: Vec<u32>,
    /// A Combustion module is present: the stage simulates temperature and fuel.
    fire: bool,
    /// MacCormack advection: each advection is followed by its correction.
    sharp: bool,
    /// Collider modules are present: solids are marked each tick.
    colliders: bool,
}

/// Packs the stage constants `solver.wgsl` reads.
fn pack_constants(modules: &[ExtensionModulePlan]) -> Result<PackedStage, String> {
    let of = |type_id: &'static str| modules_of(modules, type_id);

    let mut grids = of(MODULE_GRID);
    let grid = grids
        .next()
        .ok_or("a Fluid Solver stage needs a Fluid Grid module")?;
    if grids.next().is_some() {
        return Err("a Fluid Solver stage takes one Fluid Grid module".into());
    }
    let sources: Vec<_> = of(MODULE_DENSITY_SOURCE).collect();
    if sources.len() > MAX_SOURCES {
        return Err(format!(
            "a Fluid Solver stage takes at most {MAX_SOURCES} density sources, got {}",
            sources.len()
        ));
    }
    let strength = |type_id: &'static str| {
        of(type_id)
            .map(|module| scalar(&module.parameters, "strength"))
            .sum::<Result<f32, String>>()
    };

    let resolution = count(&grid.parameters, "resolution")?;
    let iterations = count(&grid.parameters, "pressure_iterations")?;
    let cell_size = scalar(&grid.parameters, "cell_size")?;
    let center = vec3(&grid.parameters, "center")?;
    let half_extent = resolution as f32 * cell_size * 0.5;
    let origin = center.map(|axis| axis - half_extent);
    let mut words = vec![0u32; SOURCE_BASE + sources.len() * SOURCE_WORDS];
    words[0] = resolution;
    words[1] = cell_size.to_bits();
    for axis in 0..3 {
        words[2 + axis] = origin[axis].to_bits();
    }
    words[5] = scalar(&grid.parameters, "density_dissipation")?.to_bits();
    words[6] = scalar(&grid.parameters, "velocity_dissipation")?.to_bits();
    words[7] = strength(MODULE_BUOYANCY)?.to_bits();
    words[8] = strength(MODULE_VORTICITY)?.to_bits();
    words[9] = sources.len() as u32;
    words[10] = open_side_mask(&grid.parameters);
    // MacCormack advection (fluid F4); off falls back to plain semi-Lagrangian.
    words[11] = u32::from(grid.parameters.get_bool("sharp_advection").unwrap_or(true));
    // Turbulence: strength (0 without the module), noise frequency, evolution rate, mask flag.
    let mut turbulences = of(MODULE_TURBULENCE);
    if let Some(turbulence) = turbulences.next() {
        words[14] = scalar(&turbulence.parameters, "strength")?.to_bits();
        words[15] = (1.0 / scalar(&turbulence.parameters, "scale")?).to_bits();
        words[16] = scalar(&turbulence.parameters, "evolution")?.to_bits();
        words[17] = u32::from(turbulence.parameters.get_bool("masked").unwrap_or(true));
    }
    if turbulences.next().is_some() {
        return Err("a Fluid Solver stage takes one Turbulence module".into());
    }
    for (index, source) in sources.iter().enumerate() {
        let base = SOURCE_BASE + index * SOURCE_WORDS;
        let position = vec3(&source.parameters, "position")?;
        let velocity = vec3(&source.parameters, "velocity")?;
        for axis in 0..3 {
            words[base + axis] = position[axis].to_bits();
            words[base + 4 + axis] = velocity[axis].to_bits();
        }
        words[base + 3] = scalar(&source.parameters, "radius")?.to_bits();
        words[base + 7] = scalar(&source.parameters, "density_rate")?.to_bits();
        words[base + 8..base + 11].copy_from_slice(&host_ref(source.host_fields.get("position"))?);
        words[base + 11..base + 14].copy_from_slice(&host_ref(source.host_fields.get("velocity"))?);
        words[base + 14] = scalar(&source.parameters, "temperature_rate")?.to_bits();
        words[base + 15] = scalar(&source.parameters, "fuel_rate")?.to_bits();
    }
    // The Combustion block follows the sources (see `combustion` in solver.wgsl).
    let mut combustions = of(MODULE_COMBUSTION);
    let combustion = combustions.next();
    if combustions.next().is_some() {
        return Err("a Fluid Solver stage takes one Combustion module".into());
    }
    if let Some(combustion) = combustion {
        for name in COMBUSTION_INPUTS {
            words.push(scalar(&combustion.parameters, name)?.to_bits());
        }
    }
    // Colliders (fluid F4) follow: their count and first word in the header, then one record each.
    let colliders: Vec<_> = modules
        .iter()
        .filter(|module| is_collider(&module.module_type.0))
        .collect();
    if colliders.len() > MAX_COLLIDERS {
        return Err(format!(
            "a Fluid Solver stage takes at most {MAX_COLLIDERS} colliders, got {}",
            colliders.len()
        ));
    }
    words[12] = colliders.len() as u32;
    words[13] = words.len() as u32;
    for collider in &colliders {
        words.extend(pack_collider(collider)?);
    }
    Ok(PackedStage {
        resolution,
        iterations,
        cell_size,
        origin,
        sharp: words[11] != 0,
        colliders: !colliders.is_empty(),
        constants: words,
        fire: combustion.is_some(),
    })
}

/// Packs a Volume Look's inputs into the constant words `volume.wgsl` reads.
fn pack_volume(payload: &PropertyBag) -> Result<Vec<u32>, String> {
    let steps = count(payload, "steps")?;
    if !(1..=MAX_VOLUME_STEPS).contains(&steps) {
        return Err(format!(
            "march steps must be between 1 and {MAX_VOLUME_STEPS}, got {steps}"
        ));
    }
    let shadow_steps = count(payload, "shadow_steps")?;
    if shadow_steps > MAX_SHADOW_STEPS {
        return Err(format!(
            "shadow steps must be at most {MAX_SHADOW_STEPS}, got {shadow_steps}"
        ));
    }
    let non_negative = |name: &str| {
        let value = scalar(payload, name)?;
        if value < 0.0 {
            return Err(format!("'{name}' must not be negative"));
        }
        Ok(value)
    };
    let direction = vec3(payload, "light_direction")?;
    let length = direction.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
    if length <= 1e-6 {
        return Err("the light direction must not be zero".into());
    }
    let mut words = vec![0u32; 16];
    words[0] = steps;
    words[1] = shadow_steps;
    words[2] = non_negative("opacity")?.to_bits();
    words[3] = non_negative("ambient")?.to_bits();
    for (axis, value) in vec3(payload, "color")?.into_iter().enumerate() {
        words[4 + axis] = value.to_bits();
    }
    words[7] = non_negative("light_intensity")?.to_bits();
    for axis in 0..3 {
        words[8 + axis] = (direction[axis] / length).to_bits();
    }
    for (axis, value) in vec3(payload, "light_color")?.into_iter().enumerate() {
        words[12 + axis] = value.to_bits();
    }
    // Fire: which field slot holds the temperature (none until `present` binds it), its glow, and
    // how many kelvin one unit of temperature stands for.
    words.extend([
        NO_SLOT,
        non_negative("fire_intensity")?.to_bits(),
        non_negative("temperature_scale")?.to_bits(),
    ]);
    // The grid's open sides (none until `present` reads them from the Fluid Grid) and the depth of
    // the fade in from each.
    let edge_fade = non_negative("edge_fade")?;
    if edge_fade > 0.5 {
        return Err(format!(
            "the open edge fade must be at most 0.5, got {edge_fade}"
        ));
    }
    words.extend([0, edge_fade.to_bits()]);
    Ok(words)
}

/// The volume look's constant words holding the temperature field's slot and the open-side mask.
const VOLUME_TEMPERATURE_SLOT: usize = 16;
const VOLUME_OPEN_SIDES: usize = 19;

/// Lowers a Fluid Solver stage into the solver's passes (see the crate docs).
struct FluidSolverLowerer;

impl StageLowerer for FluidSolverLowerer {
    /// A Volume Look draws the density grid as lit smoke, and the temperature grid — when the stage
    /// burns — as blackbody fire.
    fn present(
        &self,
        input: &StageLoweringInput<'_>,
        block: &ExecutionBlock,
    ) -> Result<Vec<StagePresentation>, String> {
        let temperature = ResourceTypeId::new(RESOURCE_TEMPERATURE);
        let fire = block.field(&temperature).is_some();
        let open_sides = modules_of(input.modules, MODULE_GRID)
            .next()
            .map_or(0, |grid| open_side_mask(&grid.parameters));
        modules_of(input.modules, MODULE_VOLUME_LOOK)
            .map(|look| {
                let mut fields = vec![ResourceTypeId::new(RESOURCE_DENSITY)];
                let mut constants = pack_volume(&look.parameters)?;
                constants[VOLUME_OPEN_SIDES] = open_sides;
                if fire {
                    constants[VOLUME_TEMPERATURE_SLOT] = fields.len() as u32;
                    fields.push(temperature.clone());
                }
                Ok(StagePresentation::Volume(VolumePresentation {
                    program: ComputeProgramId::new(PROGRAM_VOLUME),
                    entry_point: VOLUME_ENTRY.into(),
                    fields,
                    constants,
                }))
            })
            .collect()
    }

    fn lower(&self, input: &StageLoweringInput<'_>) -> Result<ExecutionBlock, String> {
        let PackedStage {
            resolution,
            iterations,
            cell_size,
            origin,
            constants,
            fire,
            sharp,
            colliders,
        } = pack_constants(input.modules)?;
        let groups = resolution / WORKGROUP;
        let dispatch = StagedDispatch {
            x: groups,
            y: groups,
            z: groups,
        };
        let pass = |entry: &str, accesses: Vec<ResourceAccess>| {
            ExecutionOp::Compute(ComputeOp {
                name: format!("fluid/{entry}"),
                program: Some(ComputeProgramId::new(PROGRAM_SOLVER)),
                entry_point: entry.into(),
                accesses,
                dispatch,
            })
        };
        let copy = |from: &str, to: &str| {
            ExecutionOp::Copy(CopyOp {
                from: ResourceTypeId::new(from),
                to: ResourceTypeId::new(to),
            })
        };
        let read = ResourceAccess::read;
        let write = ResourceAccess::write;
        let read_write = ResourceAccess::read_write;
        let constants_read = || read(AESTRA_RESOURCE_STAGE_CONSTANTS);
        let frame_read = || read(AESTRA_RESOURCE_FRAME);
        // After a scalar's semi-Lagrangian step (into `scratch`): with MacCormack, its correction into
        // the scalar hat and a copy back; otherwise the copy back.
        let settle = |correction: &str, field: &'static str, scratch: &'static str| {
            if sharp {
                vec![
                    pass(
                        correction,
                        vec![
                            read(RESOURCE_VELOCITY),
                            read(field),
                            read(scratch),
                            write(RESOURCE_SCALAR_HAT),
                            constants_read(),
                            frame_read(),
                        ],
                    ),
                    copy(RESOURCE_SCALAR_HAT, field),
                ]
            } else {
                vec![copy(scratch, field)]
            }
        };

        let mut steps = Vec::new();
        if colliders {
            // Solids first: every later pass sees this tick's colliders.
            steps.push(pass(
                "mark_solids",
                vec![
                    write(RESOURCE_SOLID),
                    read_write(RESOURCE_DENSITY),
                    constants_read(),
                    frame_read(),
                    read(AESTRA_RESOURCE_HOST_BINDINGS),
                ],
            ));
        }
        steps.push(pass(
            "add_sources",
            vec![
                read_write(RESOURCE_VELOCITY),
                read_write(RESOURCE_DENSITY),
                constants_read(),
                frame_read(),
                read(AESTRA_RESOURCE_HOST_BINDINGS),
            ],
        ));
        if fire {
            steps.push(pass(
                "add_heat",
                vec![
                    read_write(RESOURCE_TEMPERATURE),
                    read_write(RESOURCE_FUEL),
                    constants_read(),
                    frame_read(),
                    read(AESTRA_RESOURCE_HOST_BINDINGS),
                ],
            ));
            steps.push(pass(
                "combust",
                vec![
                    read_write(RESOURCE_DENSITY),
                    read_write(RESOURCE_TEMPERATURE),
                    read_write(RESOURCE_FUEL),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(pass(
                "apply_buoyancy_fire",
                vec![
                    read_write(RESOURCE_VELOCITY),
                    read(RESOURCE_DENSITY),
                    read(RESOURCE_TEMPERATURE),
                    constants_read(),
                    frame_read(),
                ],
            ));
        } else {
            steps.push(pass(
                "apply_buoyancy",
                vec![
                    read_write(RESOURCE_VELOCITY),
                    read(RESOURCE_DENSITY),
                    constants_read(),
                    frame_read(),
                ],
            ));
        }
        if input
            .modules
            .iter()
            .any(|module| module.module_type.0.as_str() == MODULE_VORTICITY)
        {
            steps.push(pass(
                "compute_vorticity",
                vec![
                    read(RESOURCE_VELOCITY),
                    write(RESOURCE_VORTICITY),
                    constants_read(),
                ],
            ));
            // The confinement force per cell goes to the velocity scratch, then onto the faces.
            steps.push(pass(
                "vorticity_force",
                vec![
                    read(RESOURCE_VORTICITY),
                    write(RESOURCE_VELOCITY_NEXT),
                    constants_read(),
                ],
            ));
            steps.push(pass(
                "apply_vorticity",
                vec![
                    read_write(RESOURCE_VELOCITY),
                    read(RESOURCE_VELOCITY_NEXT),
                    constants_read(),
                    frame_read(),
                ],
            ));
        }
        steps.push(pass(
            "advect_velocity",
            vec![
                read(RESOURCE_VELOCITY),
                write(RESOURCE_VELOCITY_NEXT),
                constants_read(),
                frame_read(),
            ],
        ));
        if sharp {
            // MacCormack: correct the semi-Lagrangian result, then take the corrected one.
            steps.push(pass(
                "correct_velocity",
                vec![
                    read(RESOURCE_VELOCITY),
                    read(RESOURCE_VELOCITY_NEXT),
                    write(RESOURCE_VELOCITY_HAT),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(copy(RESOURCE_VELOCITY_HAT, RESOURCE_VELOCITY));
        } else {
            steps.push(copy(RESOURCE_VELOCITY_NEXT, RESOURCE_VELOCITY));
        }
        steps.push(pass(
            "compute_divergence",
            vec![
                read(RESOURCE_VELOCITY),
                write(RESOURCE_DIVERGENCE),
                read(RESOURCE_SOLID),
                constants_read(),
            ],
        ));
        steps.push(ExecutionOp::Repeat {
            policy: RepeatPolicy::FixedCount(iterations),
            body: with_barriers(vec![
                pass(
                    "relax_pressure",
                    vec![
                        read(RESOURCE_PRESSURE),
                        write(RESOURCE_PRESSURE_NEXT),
                        read(RESOURCE_DIVERGENCE),
                        read(RESOURCE_SOLID),
                        constants_read(),
                    ],
                ),
                copy(RESOURCE_PRESSURE_NEXT, RESOURCE_PRESSURE),
                // Closes the iteration: the next relax reads this copy.
                ExecutionOp::Barrier,
            ]),
        });
        steps.push(pass(
            "project",
            vec![
                read_write(RESOURCE_VELOCITY),
                read(RESOURCE_PRESSURE),
                read(RESOURCE_SOLID),
                constants_read(),
            ],
        ));
        steps.push(pass(
            "advect_density",
            vec![
                read(RESOURCE_VELOCITY),
                read(RESOURCE_DENSITY),
                write(RESOURCE_DENSITY_NEXT),
                constants_read(),
                frame_read(),
            ],
        ));
        steps.extend(settle(
            "correct_density",
            RESOURCE_DENSITY,
            RESOURCE_DENSITY_NEXT,
        ));
        let mut fields = vec![(RESOURCE_VELOCITY, 4), (RESOURCE_DENSITY, 1)];
        if fire {
            for (entry, correction, field, scratch) in [
                (
                    "advect_temperature",
                    "correct_temperature",
                    RESOURCE_TEMPERATURE,
                    RESOURCE_TEMPERATURE_NEXT,
                ),
                (
                    "advect_fuel",
                    "correct_fuel",
                    RESOURCE_FUEL,
                    RESOURCE_FUEL_NEXT,
                ),
            ] {
                steps.push(pass(
                    entry,
                    vec![
                        read(RESOURCE_VELOCITY),
                        read(field),
                        write(scratch),
                        constants_read(),
                        frame_read(),
                    ],
                ));
                steps.extend(settle(correction, field, scratch));
                fields.push((field, 1));
            }
        }

        Ok(ExecutionBlock {
            resources: resources(resolution, constants.len(), fire),
            ops: with_barriers(steps),
            constants,
            // The persistent grids, for debug views, field sampling and renderers.
            fields: fields
                .into_iter()
                .map(|(resource, components)| FieldLayout {
                    resource: ResourceTypeId::new(resource),
                    dims: [resolution; 3],
                    components,
                    origin,
                    cell_size,
                    // Velocity lives on the cells' faces (a MAC grid, fluid F4).
                    staggered: resource == RESOURCE_VELOCITY,
                })
                .collect(),
        })
    }
}

/// Every step reads what the previous one wrote, so each is separated by a barrier.
fn with_barriers(steps: Vec<ExecutionOp>) -> Vec<ExecutionOp> {
    let mut ops = Vec::with_capacity(steps.len() * 2);
    for step in steps {
        let is_barrier = matches!(step, ExecutionOp::Barrier);
        if !ops.is_empty() && !is_barrier && !matches!(ops.last(), Some(ExecutionOp::Barrier)) {
            ops.push(ExecutionOp::Barrier);
        }
        ops.push(step);
    }
    ops
}
