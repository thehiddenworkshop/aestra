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
//! - **programs**: the solver (`solver.wgsl`) whose entry points the stage lowers to — composed once
//!   with the dense grid and once with the sparse one (fluid F7: 8³-cell bricks allocated where the
//!   fluid is, so a grid up to 512³ costs what its fluid occupies) — and the volume look's march
//!   function (`volume.wgsl`);
//! - a second stage type, *Liquid Solver* (fluid F8, [`liquid`](crate::liquid_effect)): a
//!   free-surface liquid of APIC particles on the same grid, pressure solve and colliders.
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
    AESTRA_RESOURCE_WORLD_SDF, CompiledHostFieldRef, ComputeOp, CopyOp, ExecutionBlock,
    ExecutionOp, ExtensionModulePlan, FieldLayout, IndirectDispatch, QualityTier, RepeatPolicy,
    ResourceAccess, ResourceDescriptor, ResourceLifetime, StagePresentation, StagedDispatch,
    VolumePresentation,
};
use std::sync::Arc;

mod liquid;
pub use liquid::{
    CAPABILITY_LIQUID, LIQUID_ENTRY_POINTS, LIQUID_LOOK_ENTRY, LIQUID_LOOK_WGSL, LIQUID_STAGE,
    LIQUID_WGSL, MAX_LIQUID_BLOCKS, MAX_LIQUID_PARTICLES, MAX_LIQUID_SUBSTEPS, MODULE_LIQUID_BLOCK,
    MODULE_LIQUID_GRID, MODULE_LIQUID_LOOK, MODULE_LIQUID_SOURCE, PROGRAM_LIQUID,
    PROGRAM_LIQUID_LOOK, RESOURCE_LIQUID_DISPATCH, RESOURCE_LIQUID_HEADER,
    RESOURCE_LIQUID_PARTICLES, RESOURCE_LIQUID_TRANSFER, STAGE_LIQUID_SOLVER, liquid_effect,
    liquid_entry_points, liquid_program_wgsl,
};

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
pub const MODULE_SECONDARY_EMISSION: &str = "org.example.aestra-fluid::module/secondary_emission";
pub const MODULE_WORLD_COLLIDER: &str = "org.example.aestra-fluid::module/world_collider";
pub const PROGRAM_SOLVER: &str = "org.example.aestra-fluid::program/solver";
/// The same solver over a sparse grid of bricks (fluid F7), with the brick allocation.
pub const PROGRAM_SOLVER_SPARSE: &str = "org.example.aestra-fluid::program/solver_sparse";
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
/// The pressure solve's search direction p and its image A·p, side by side per cell.
pub const RESOURCE_PCG_VECTORS: &str = "org.example.aestra-fluid::resource/pcg_vectors";
pub const RESOURCE_MG_SOLUTION: &str = "org.example.aestra-fluid::resource/multigrid_solution";
pub const RESOURCE_MG_RHS: &str = "org.example.aestra-fluid::resource/multigrid_rhs";
pub const RESOURCE_MG_FLAGS: &str = "org.example.aestra-fluid::resource/multigrid_flags";
/// The pressure solve's scalars, then its per-workgroup partial sums; word 0 is the relative residual
/// its convergent repeat tests.
pub const RESOURCE_PCG_REDUCTION: &str = "org.example.aestra-fluid::resource/pcg_reduction";
/// Leapfrog flow maps (fluid F6): the cycle's midpoint velocities, its initial velocity, the forward and
/// backward maps, this tick's forces, the mapped impulse.
pub const RESOURCE_LFM_HISTORY: &str = "org.example.aestra-fluid::resource/flow_map_history";
pub const RESOURCE_LFM_INITIAL: &str = "org.example.aestra-fluid::resource/flow_map_initial";
pub const RESOURCE_LFM_FORWARD: &str = "org.example.aestra-fluid::resource/flow_map_forward";
pub const RESOURCE_LFM_BACKWARD: &str = "org.example.aestra-fluid::resource/flow_map_backward";
pub const RESOURCE_LFM_FORCE: &str = "org.example.aestra-fluid::resource/flow_map_force";
pub const RESOURCE_LFM_IMPULSE: &str = "org.example.aestra-fluid::resource/flow_map_impulse";
pub const RESOURCE_TEMPERATURE: &str = "org.example.aestra-fluid::resource/temperature_grid";
pub const RESOURCE_TEMPERATURE_NEXT: &str =
    "org.example.aestra-fluid::resource/temperature_scratch";
pub const RESOURCE_FUEL: &str = "org.example.aestra-fluid::resource/fuel_grid";
pub const RESOURCE_FUEL_NEXT: &str = "org.example.aestra-fluid::resource/fuel_scratch";
/// A sparse grid's bricks (fluid F7): the active count and list, each slot's brick and the brick
/// table; the allocation's scratch; the passes' workgroup counts.
pub const RESOURCE_BRICKS: &str = "org.example.aestra-fluid::resource/bricks";
pub const RESOURCE_BRICK_SCRATCH: &str = "org.example.aestra-fluid::resource/brick_scratch";
pub const RESOURCE_BRICK_DISPATCH: &str = "org.example.aestra-fluid::resource/brick_dispatch";
/// Secondary emission (fluid F10): the emission list a Spawn From Domain emitter reads, and its ranks.
pub const RESOURCE_EMISSION: &str = "org.example.aestra-fluid::resource/emission";
pub const RESOURCE_EMISSION_SCRATCH: &str = "org.example.aestra-fluid::resource/emission_scratch";
/// Colliders' forces (fluid F11): per-workgroup partials, and the outputs the host reads.
pub const RESOURCE_COLLIDER_FORCES: &str = "org.example.aestra-fluid::resource/collider_forces";
pub const RESOURCE_OUTPUTS: &str = "org.example.aestra-fluid::resource/outputs";
/// The output a collider reports its force as, and the event a push past its threshold raises.
pub const OUTPUT_FORCE: &str = "force";
pub const EVENT_IMPACT: &str = aestra_runtime::EVENT_IMPACT;

/// The solver's WGSL; see [`program_wgsl`] for the full program with the host-binding accessors.
pub const SOLVER_WGSL: &str = include_str!("solver.wgsl");
/// The multigrid-preconditioned conjugate-gradient pressure solve (fluid F5), after the solver.
pub const PRESSURE_WGSL: &str = include_str!("pressure.wgsl");
/// Leapfrog flow maps (fluid F6), after the solver.
pub const FLOWMAP_WGSL: &str = include_str!("flowmap.wgsl");
/// The volume look's march function, composed after the backend's volume interface.
pub const VOLUME_WGSL: &str = include_str!("volume.wgsl");
/// A gas's pressure faces all weigh 1 (fluid F9: a spatiotemporal liquid weighs them by its phase
/// field, `liquid.wgsl`).
pub const UNIT_COEFFICIENTS_WGSL: &str =
    "fn face_coefficient(cell: vec3<i32>, axis: u32) -> f32 {\n    return 1.0;\n}\n";
/// How the solver indexes its cells (fluid F7): every cell stored, or only the active bricks'.
pub const GRID_DENSE_WGSL: &str = include_str!("grid_dense.wgsl");
pub const GRID_SPARSE_WGSL: &str = include_str!("grid_sparse.wgsl");
/// Secondary emission (fluid F10): a gas's cells, and the ranking every stage shares.
pub const EMIT_WGSL: &str = include_str!("emit.wgsl");
/// A liquid's secondary emission: its particles.
pub const EMIT_LIQUID_WGSL: &str = include_str!("emit_liquid.wgsl");
/// The world collider (fluid F11): solids from the host's world SDF, in every fluid program.
pub const WORLD_WGSL: &str = include_str!("world.wgsl");
/// A liquid's particles against the world collider.
pub const WORLD_LIQUID_WGSL: &str = include_str!("world_liquid.wgsl");
/// The colliders' forces, reported to the host (fluid F11), in every fluid program.
pub const FORCES_WGSL: &str = include_str!("forces.wgsl");
/// Records a Secondary Emission module's list holds at most.
pub const MAX_EMISSION_CAPACITY: u32 = 16384;

/// The grid resolution per axis is bounded (plan §11.1): at 128³ every grid together — the solver's
/// scratch and the multigrid levels included — is ~300 MB (~330 MB with fire).
pub const MIN_RESOLUTION: u32 = 8;
pub const MAX_RESOLUTION: u32 = 128;
/// A sparse grid (fluid F7) stores 8³-cell bricks: its resolution is a multiple of the edge, up to
/// 512 (a 64³ table of bricks), and it stores at most the budget's bricks at once.
pub const BRICK_EDGE: u32 = 8;
pub const MAX_SPARSE_RESOLUTION: u32 = 512;
pub const MAX_BRICK_BUDGET: u32 = 4096;
pub const MAX_PRESSURE_ITERATIONS: u32 = 200;
/// Steps in a flow-map reinitialization cycle, and the largest grid flow maps run on: every cell keeps
/// (12·cycle + 88) bytes of map state, and ~100 more are scratch.
pub const MAX_FLOW_MAP_CYCLE: u32 = 16;
pub const MAX_FLOW_MAP_RESOLUTION: u32 = 96;
/// Density sources one stage packs into its constants.
pub const MAX_SOURCES: usize = 8;
/// The solver's workgroup edge; the resolution must be a multiple of it.
pub const WORKGROUP: u32 = 4;

/// The stage-constant layout `solver.wgsl` reads (words).
const SOURCE_BASE: usize = 32;
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
const SOURCE_WORDS: usize = 17;
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

/// The complete solver program: the solver passes on the dense grid, the pressure solve with its
/// per-level entry points, and the shared host-binding accessors and reductions they call.
pub fn program_wgsl() -> String {
    format!(
        "{SOLVER_WGSL}\n{GRID_DENSE_WGSL}\n{UNIT_COEFFICIENTS_WGSL}\n{PRESSURE_WGSL}\n{FLOWMAP_WGSL}\n{EMIT_WGSL}\n{WORLD_WGSL}\n{FORCES_WGSL}\n{}\n{}\n{}\n{}\n{}",
        multigrid_entries_wgsl(),
        aestra_gpu::HOST_BINDINGS_WGSL,
        aestra_gpu::reduce::REDUCE_WGSL,
        aestra_gpu::scan::SCAN_WGSL,
        aestra_gpu::WORLD_SDF_WGSL
    )
}

/// The same passes over a sparse grid of bricks (fluid F7), with the brick allocation and the prefix
/// sums it ranks with.
pub fn sparse_program_wgsl() -> String {
    format!(
        "{SOLVER_WGSL}\n{GRID_SPARSE_WGSL}\n{UNIT_COEFFICIENTS_WGSL}\n{PRESSURE_WGSL}\n{FLOWMAP_WGSL}\n{EMIT_WGSL}\n{WORLD_WGSL}\n{FORCES_WGSL}\n{}\n{}\n{}\n{}\n{}",
        multigrid_entries_wgsl(),
        aestra_gpu::HOST_BINDINGS_WGSL,
        aestra_gpu::reduce::REDUCE_WGSL,
        aestra_gpu::scan::SCAN_WGSL,
        aestra_gpu::WORLD_SDF_WGSL
    )
}

/// The brick allocation's entry points, in the sparse program only.
pub const BRICK_ENTRY_POINTS: [&str; 8] = [
    "brick_activity",
    "brick_activity_fire",
    "brick_need",
    "brick_plan",
    "brick_assign",
    "brick_compact",
    "brick_zero",
    "brick_zero_fire",
];

/// Levels the multigrid pressure solve uses at most: a 128-cell grid halves down to 4.
pub const MAX_MULTIGRID_LEVELS: usize = 6;

/// The multigrid levels' resolutions, finest first: they halve while the grid stays even and keeps at
/// least 3 cells a side.
pub fn multigrid_levels(resolution: u32) -> Vec<u32> {
    let mut levels = vec![resolution];
    let mut n = resolution;
    while n.is_multiple_of(2) && n / 2 >= 3 && levels.len() < MAX_MULTIGRID_LEVELS {
        n /= 2;
        levels.push(n);
    }
    levels
}

/// The V-cycle's passes, one entry point per kind and level (`pressure.wgsl` has the bodies).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LevelPass {
    SmoothRed,
    SmoothBlack,
    /// Into this level from the finer one, with this level's first red sweep.
    RestrictSmooth,
    /// From the coarser level into this one, with this level's black sweep.
    ProlongSmooth,
    /// This level's solid flags: the fine level's from the solids, a coarse one's from the finer.
    Coarsen,
}

impl LevelPass {
    const ALL: [Self; 5] = [
        Self::SmoothRed,
        Self::SmoothBlack,
        Self::RestrictSmooth,
        Self::ProlongSmooth,
        Self::Coarsen,
    ];

    fn levels(self) -> std::ops::Range<usize> {
        match self {
            Self::RestrictSmooth => 1..MAX_MULTIGRID_LEVELS,
            Self::ProlongSmooth => 0..MAX_MULTIGRID_LEVELS - 1,
            Self::SmoothRed | Self::SmoothBlack | Self::Coarsen => 0..MAX_MULTIGRID_LEVELS,
        }
    }

    fn entry(self, level: usize) -> String {
        let kind = match self {
            Self::SmoothRed => "smooth_red",
            Self::SmoothBlack => "smooth_black",
            Self::RestrictSmooth => "restrict",
            Self::ProlongSmooth => "prolong",
            Self::Coarsen => "coarsen",
        };
        format!("mg_{kind}_{level}")
    }

    /// The call an entry makes for the cell `cell` of its level.
    fn body(self, level: usize) -> String {
        match self {
            Self::SmoothRed => format!("mg_smooth({level}u, cell, 0u)"),
            Self::SmoothBlack => format!("mg_smooth({level}u, cell, 1u)"),
            Self::RestrictSmooth => format!("mg_restrict_smooth({level}u, cell)"),
            Self::ProlongSmooth => format!("mg_prolong_smooth({level}u, cell)"),
            Self::Coarsen => format!("mg_coarsen({level}u, cell)"),
        }
    }
}

/// One thin entry point per V-cycle pass and level: the level is a constant of the entry, since a
/// dispatch carries no parameters of its own.
fn multigrid_entries_wgsl() -> String {
    let mut wgsl = String::new();
    for pass in LevelPass::ALL {
        for level in pass.levels() {
            wgsl.push_str(&format!(
                "@compute @workgroup_size(4, 4, 4)\nfn {}(@builtin(global_invocation_id) gid: \
                 vec3<u32>) {{\n    let cell = lv_cell({level}u, gid);\n    {};\n}}\n",
                pass.entry(level),
                pass.body(level)
            ));
        }
    }
    wgsl
}

/// The secondary emission's (fluid F10) and the world collider's (fluid F11) entry points, in every
/// fluid program.
pub const EMIT_ENTRY_POINTS: [&str; 7] = [
    "emit_cells",
    "emit_cells_fire",
    "emit_cells_write",
    "emit_offsets",
    "mark_world_solids",
    // The colliders' forces (fluid F11).
    "measure_collider_forces",
    "collider_force_total",
];

/// Every entry point of the solver program: [`ENTRY_POINTS`], the secondary emission's and the
/// per-level V-cycle passes.
pub fn entry_points() -> Vec<String> {
    let mut entries: Vec<String> = ENTRY_POINTS
        .iter()
        .chain(&EMIT_ENTRY_POINTS)
        .map(|entry| entry.to_string())
        .collect();
    for pass in LevelPass::ALL {
        entries.extend(pass.levels().map(|level| pass.entry(level)));
    }
    entries
}

/// Every entry point of the sparse solver program: the solver's and the brick allocation's.
pub fn sparse_entry_points() -> Vec<String> {
    let mut entries = entry_points();
    entries.extend(BRICK_ENTRY_POINTS.iter().map(|entry| entry.to_string()));
    entries
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

/// The solver's hand-written entry points, as its compute ops name them (see [`entry_points`] for all).
pub const ENTRY_POINTS: [&str; 41] = [
    "add_sources",
    "compute_vorticity",
    "vorticity_force",
    "apply_vorticity",
    "advect_velocity",
    "advect_density",
    "compute_divergence",
    "relax_pressure",
    "relax_pressure_back",
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
    "pcg_setup",
    "pcg_setup_finalize",
    "pcg_start",
    "pcg_smooth_dot",
    "pcg_beta",
    "pcg_apply",
    "pcg_alpha",
    "pcg_step",
    "pcg_residual",
    "lfm_forces",
    "lfm_advect",
    "lfm_march_forward",
    "lfm_pull_back",
    "lfm_measure_error",
    "lfm_compensate",
    "lfm_impulse_divergence",
    "lfm_project",
    "lfm_energy",
    "lfm_energy_total",
    "lfm_restart",
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
        let liquid_capability = CapabilityId::new(CAPABILITY_LIQUID);
        registry.register_capability(liquid_capability.clone())?;
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
            (RESOURCE_PRESSURE, "Pressure", ResourceLifetime::Persistent),
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
                RESOURCE_PCG_VECTORS,
                "Pressure Search Direction",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_MG_SOLUTION,
                "Multigrid Solution",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_MG_RHS,
                "Multigrid Right-Hand Side",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_MG_FLAGS,
                "Multigrid Solid Flags",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_PCG_REDUCTION,
                "Pressure Sums",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_LFM_HISTORY,
                "Flow Map Velocities",
                ResourceLifetime::Persistent,
            ),
            (
                RESOURCE_LFM_INITIAL,
                "Flow Map Initial Velocity",
                ResourceLifetime::Persistent,
            ),
            (
                RESOURCE_LFM_FORWARD,
                "Forward Flow Map",
                ResourceLifetime::Persistent,
            ),
            (
                RESOURCE_LFM_BACKWARD,
                "Backward Flow Map",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_LFM_FORCE,
                "Flow Map Forces",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_LFM_IMPULSE,
                "Flow Map Impulse",
                ResourceLifetime::Transient,
            ),
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
            (RESOURCE_BRICKS, "Bricks", ResourceLifetime::Persistent),
            (
                RESOURCE_BRICK_SCRATCH,
                "Brick Allocation",
                ResourceLifetime::Transient,
            ),
            (
                RESOURCE_BRICK_DISPATCH,
                "Brick Workgroups",
                ResourceLifetime::Persistent,
            ),
            (RESOURCE_EMISSION, "Emission", ResourceLifetime::Transient),
            (
                RESOURCE_COLLIDER_FORCES,
                "Collider Forces",
                ResourceLifetime::Transient,
            ),
            (RESOURCE_OUTPUTS, "Outputs", ResourceLifetime::Persistent),
            (
                RESOURCE_EMISSION_SCRATCH,
                "Emission Ranks",
                ResourceLifetime::Transient,
            ),
        ]
        .into_iter()
        .chain(liquid::LIQUID_RESOURCES)
        {
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
        registry.register_stage(StageTypeDescriptor::gpu_only(
            StageTypeId::new(STAGE_LIQUID_SOLVER),
            "Liquid Solver",
            CapabilitySet::new([liquid_capability.clone()]),
        ))?;
        let requires = CapabilityExpression::AnyOf(CapabilitySet::new([fluid_grid.clone()]));
        let liquid_requires =
            CapabilityExpression::AnyOf(CapabilitySet::new([liquid_capability.clone()]));
        // Colliders serve a gas and a liquid alike.
        let either =
            CapabilityExpression::AnyOf(CapabilitySet::new([fluid_grid, liquid_capability]));
        for metadata in [
            grid_metadata(requires.clone()),
            density_source_metadata(requires.clone()),
            buoyancy_metadata(requires.clone()),
            vorticity_metadata(requires.clone()),
            turbulence_metadata(requires.clone()),
            combustion_metadata(requires.clone()),
            collider_metadata(MODULE_SPHERE_COLLIDER, either.clone()),
            collider_metadata(MODULE_BOX_COLLIDER, either.clone()),
            collider_metadata(MODULE_CAPSULE_COLLIDER, either.clone()),
            secondary_emission_metadata(either.clone()),
            world_collider_metadata(either),
            volume_look_metadata(requires),
            liquid::liquid_grid_metadata(liquid_requires.clone()),
            liquid::liquid_block_metadata(liquid_requires.clone()),
            liquid::liquid_source_metadata(liquid_requires.clone()),
            liquid::liquid_look_metadata(liquid_requires),
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
            entry_points: entry_points(),
        })?;
        registry.register_program(ComputeProgram {
            id: ComputeProgramId::new(PROGRAM_SOLVER_SPARSE),
            wgsl: sparse_program_wgsl(),
            entry_points: sparse_entry_points(),
        })?;
        registry.register_program(ComputeProgram {
            id: ComputeProgramId::new(PROGRAM_LIQUID),
            wgsl: liquid_program_wgsl(),
            entry_points: liquid_entry_points(),
        })?;
        registry.register_program(ComputeProgram {
            id: ComputeProgramId::new(PROGRAM_LIQUID_LOOK),
            wgsl: LIQUID_LOOK_WGSL.into(),
            entry_points: vec![LIQUID_LOOK_ENTRY.into()],
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
        registry.lowering.register_stage(
            StageTypeId::new(STAGE_LIQUID_SOLVER),
            Arc::new(liquid::LiquidSolverLowerer),
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
            "With Multigrid Pressure, the most iterations a tick's pressure solve may take before it \
             stops short of the tolerance (each unused one still costs a little); without, the Jacobi \
             sweeps it always takes.",
            Value::U32(12),
            number(1.0, 1.0, Some(MAX_PRESSURE_ITERATIONS as f32)),
        ),
        InputMetadata::new(
            "multigrid_pressure",
            "Multigrid Pressure",
            "Solves the pressure with multigrid-preconditioned conjugate gradients, until it is \
             accurate to the tolerance: the fluid stays incompressible at any resolution. Off, a \
             fixed number of Jacobi sweeps: cheaper on small grids, but leaves compression behind on \
             large ones.",
            Value::Bool(true),
            InputControl::Toggle,
        ),
        InputMetadata::new(
            "pressure_tolerance",
            "Pressure Tolerance",
            "With Multigrid Pressure, how small the solve's remaining error must be, relative to \
             where it started, for it to stop.",
            Value::Scalar(1e-3),
            number(0.0001, 0.0, Some(1.0)),
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
        InputMetadata::new(
            "flow_map",
            "Flow Map",
            "Leapfrog flow maps: carries the velocity along long-range flow maps, so swirls and vortex \
             rings live far longer than with any advection. Costs memory — (12 × cycle + 88) bytes of \
             state per cell, ~100 more of scratch — and a heavier tick at each cycle's end. Grids up \
             to 96 cells a side. (Smoke and heat keep their own advection.)",
            Value::Bool(false),
            InputControl::Toggle,
        ),
        InputMetadata::new(
            "flow_map_cycle",
            "Flow Map Cycle",
            "Steps between the flow maps' reinitializations: longer keeps more swirl, costs more memory \
             and makes the cycle's last tick heavier.",
            Value::U32(8),
            number(1.0, 1.0, Some(MAX_FLOW_MAP_CYCLE as f32)),
        ),
        InputMetadata::new(
            "sparse",
            "Sparse Bricks",
            "Stores and simulates only the 8³-cell bricks that hold smoke, heat or fuel, or touch a \
             source, and the ring of bricks around them: a large grid then costs what its fluid \
             occupies. The resolution may reach 512 cells a side (a multiple of 8). Outside the active \
             bricks is still, open air. Without flow maps. (For the volume march, raise March Steps \
             with the resolution: the steps span the whole grid.)",
            Value::Bool(false),
            InputControl::Toggle,
        ),
        InputMetadata::new(
            "brick_budget",
            "Brick Budget",
            "With Sparse Bricks, the most bricks stored at once, each 512 cells (about 65 KB of state \
             and scratch; 73 KB with fire). Fluid that would need more waits as open air until \
             bricks free.",
            Value::U32(2048),
            number(1.0, 1.0, Some(MAX_BRICK_BUDGET as f32)),
        ),
        InputMetadata::new(
            "brick_threshold",
            "Brick Threshold",
            "With Sparse Bricks, how much smoke, heat or fuel a cell must hold to keep its brick \
             active. A negative threshold keeps every brick: the grid is then stored whole (as \
             many bricks as the budget allows).",
            Value::Scalar(1e-3),
            InputControl::Number {
                step: 0.0001,
                min: None,
                max: None,
            },
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
        InputMetadata::new(
            "duration",
            "Duration",
            "How long the source runs from the effect's start — a burst, say; 0 runs it throughout.",
            Value::Scalar(0.0),
            number(0.05, 0.0, None),
        )
        .with_unit("s"),
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
    inputs.push(
        InputMetadata::new(
            "impact_threshold",
            "Impact Threshold",
            "The force the fluid must push the collider with to tell gameplay of an impact; 0 tells \
             it nothing (the force is reported either way).",
            Value::Scalar(0.0),
            number(1.0, 0.0, None),
        )
        .with_unit("force"),
    );
    fluid_module(type_id, display_name, description, requires)
        .with_inputs(inputs)
        .with_cost(2)
}

fn world_collider_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    fluid_module(
        MODULE_WORLD_COLLIDER,
        "World Collider",
        "The scene the host supplies — level geometry as a signed distance field — which the fluid \
         flows around and a liquid's particles cannot enter. Without a world from the host, nothing \
         collides.",
        requires,
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![
        InputMetadata::new(
            "offset",
            "Offset",
            "How far outside the geometry's surface the fluid stops: positive thickens it, negative \
             thins it.",
            Value::Scalar(0.0),
            InputControl::Number {
                step: 0.1,
                min: None,
                max: None,
            },
        )
        .with_unit("units"),
        InputMetadata::new(
            "no_slip",
            "Sticky",
            "The fluid sticks to the geometry (no slip) instead of sliding along it.",
            Value::Bool(false),
            InputControl::Toggle,
        ),
    ])
    .with_cost(2)
}

/// Packs a stage's World Collider (fluid F11), at most one, into header words 24–26: present, surface
/// offset, sticky. False without one.
fn pack_world(modules: &[ExtensionModulePlan], words: &mut [u32]) -> Result<bool, String> {
    let mut worlds = modules_of(modules, MODULE_WORLD_COLLIDER);
    let Some(world) = worlds.next() else {
        return Ok(false);
    };
    if worlds.next().is_some() {
        return Err("a stage takes one World Collider module".into());
    }
    let offset = scalar(&world.parameters, "offset")?;
    if !offset.is_finite() {
        return Err("the world collider's offset must be finite".into());
    }
    words[24] = 1;
    words[25] = offset.to_bits();
    words[26] = u32::from(world.parameters.get_bool("no_slip").unwrap_or(false));
    Ok(true)
}

fn secondary_emission_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    fluid_module(
        MODULE_SECONDARY_EMISSION,
        "Secondary Emission",
        "Asks for particles where the fluid is lively — a gas's hot or dense cells, a liquid's spray \
         — for an emitter's Spawn From Domain to turn into sparks, embers, droplets or mist.",
        requires,
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![
        InputMetadata::new(
            "rate",
            "Rate",
            "How often each qualifying cell (or liquid particle) asks for a particle, per second.",
            Value::Scalar(1.0),
            number(0.1, 0.0, None),
        )
        .with_unit("1/s"),
        InputMetadata::new(
            "threshold",
            "Threshold",
            "In a gas, the density — or, burning, the temperature — a cell must reach; in a liquid, \
             the liquid fraction a particle's cell must stay under (spray is clear of the liquid).",
            Value::Scalar(0.5),
            number(0.05, 0.0, None),
        ),
        InputMetadata::new(
            "min_speed",
            "Minimum Speed",
            "How fast the fluid must move there.",
            Value::Scalar(0.0),
            number(0.5, 0.0, None),
        )
        .with_unit("units/s"),
        InputMetadata::new(
            "capacity",
            "Capacity",
            "The most particles asked for in one tick; past it, the cells (or liquid particles) with \
             the lowest indices win.",
            Value::U32(1024),
            number(64.0, 1.0, Some(MAX_EMISSION_CAPACITY as f32)),
        ),
    ])
    .with_cost(2)
}

/// What a quality tier (fluid F12) does to one grid: its resolution, scaled to a valid multiple and
/// never below the minimum, and how much each cell grows so the box keeps its size.
struct TierScale {
    resolution: u32,
    cell_growth: f32,
}

impl TierScale {
    fn of(tier: &QualityTier, authored: u32, sparse: bool) -> Self {
        let multiple = if sparse { BRICK_EDGE } else { WORKGROUP };
        let resolution =
            QualityTier::scale_count(authored, tier.resolution, multiple, MIN_RESOLUTION);
        Self {
            resolution,
            cell_growth: authored as f32 / resolution as f32,
        }
    }

    /// A brick budget for the coarser grid: the same region needs fewer bricks.
    fn bricks(&self, authored: u32) -> u32 {
        ((authored as f32 / self.cell_growth.powi(3)).ceil() as u32).max(1)
    }
}

/// A Secondary Emission's constant block (fluid F10), as `emit.wgsl` reads it: rate, minimum speed,
/// threshold, capacity, and the workgroups of candidates its mask pass runs at most.
fn pack_emission(
    module: &ExtensionModulePlan,
    groups: u64,
    scaled: &TierScale,
    tier: &QualityTier,
) -> Result<[u32; 5], String> {
    let parameters = &module.parameters;
    let capacity = count(parameters, "capacity")?;
    if !(1..=MAX_EMISSION_CAPACITY).contains(&capacity) {
        return Err(format!(
            "a secondary emission's capacity must be between 1 and {MAX_EMISSION_CAPACITY}, got \
             {capacity}"
        ));
    }
    let [rate, min_speed, threshold] =
        ["rate", "min_speed", "threshold"].map(|name| scalar(parameters, name));
    let (rate, min_speed, threshold) = (rate?, min_speed?, threshold?);
    // A tier asks for its share of the particles: the list's capacity scales, and each candidate
    // (fewer, bigger cells; fewer liquid particles) asks more often to make up for their number.
    let capacity = QualityTier::scale_count(capacity, tier.particles, 1, 1);
    let rate = rate * scaled.cell_growth.powi(3) * tier.particles;
    if rate < 0.0 || min_speed < 0.0 {
        return Err("a secondary emission's rate and minimum speed must not be negative".into());
    }
    Ok([
        rate.to_bits(),
        min_speed.to_bits(),
        threshold.to_bits(),
        capacity,
        u32::try_from(groups).map_err(|_| "too many emission candidates")?,
    ])
}

/// A stage's Secondary Emission (fluid F10), at most one.
fn emission_module(
    modules: &[ExtensionModulePlan],
) -> Result<Option<&ExtensionModulePlan>, String> {
    let mut emissions = modules_of(modules, MODULE_SECONDARY_EMISSION);
    let emission = emissions.next();
    if emissions.next().is_some() {
        return Err("a stage takes one Secondary Emission module".into());
    }
    Ok(emission)
}

/// The optional resources past the first 28, in binding order: the fire grids, the liquid's, the
/// emission's (fluid F10) and the world SDF (fluid F11). A stage declaring a later one declares every
/// earlier one — a 16-byte stand-in where it does not use it — so each keeps its binding.
const OPTIONAL_RESOURCES: [&str; 13] = [
    RESOURCE_TEMPERATURE,
    RESOURCE_TEMPERATURE_NEXT,
    RESOURCE_FUEL,
    RESOURCE_FUEL_NEXT,
    RESOURCE_LIQUID_PARTICLES,
    RESOURCE_LIQUID_HEADER,
    RESOURCE_LIQUID_DISPATCH,
    RESOURCE_LIQUID_TRANSFER,
    RESOURCE_EMISSION,
    RESOURCE_EMISSION_SCRATCH,
    AESTRA_RESOURCE_WORLD_SDF,
    RESOURCE_COLLIDER_FORCES,
    RESOURCE_OUTPUTS,
];

/// Stand-ins for the optional resources up to binding `binding`.
fn pad_resources(resources: &mut Vec<ResourceDescriptor>, binding: usize) {
    while resources.len() < binding {
        resources.push(ResourceDescriptor {
            id: ResourceTypeId::new(OPTIONAL_RESOURCES[resources.len() - 28]),
            bytes: 16,
            lifetime: ResourceLifetime::Transient,
        });
    }
}

/// The host's world SDF (fluid F11), at binding 38: sized and written by the host.
fn world_resource(resources: &mut Vec<ResourceDescriptor>) {
    pad_resources(resources, 38);
    resources.push(ResourceDescriptor {
        id: ResourceTypeId::new(AESTRA_RESOURCE_WORLD_SDF),
        bytes: 0,
        lifetime: ResourceLifetime::Persistent,
    });
}

/// The colliders' forces (fluid F11): the partials at binding 39, the outputs the host reads at 40 —
/// one per collider, in module order, raising `impact` past its threshold.
fn force_resources(
    resources: &mut Vec<ResourceDescriptor>,
    colliders: &[&ExtensionModulePlan],
    groups: u64,
) -> Result<Vec<aestra_runtime::StageOutput>, String> {
    pad_resources(resources, 39);
    resources.push(ResourceDescriptor {
        id: ResourceTypeId::new(RESOURCE_COLLIDER_FORCES),
        bytes: groups * MAX_COLLIDERS as u64 * 16,
        lifetime: ResourceLifetime::Transient,
    });
    resources.push(ResourceDescriptor {
        id: ResourceTypeId::new(RESOURCE_OUTPUTS),
        bytes: MAX_COLLIDERS as u64 * FORCE_RECORD_WORDS * 4,
        lifetime: ResourceLifetime::Persistent,
    });
    colliders
        .iter()
        .enumerate()
        .map(|(index, collider)| {
            let threshold = scalar(&collider.parameters, "impact_threshold")?;
            if !(threshold.is_finite() && threshold >= 0.0) {
                return Err("a collider's impact threshold must be finite and not negative".into());
            }
            Ok(aestra_runtime::StageOutput {
                name: OUTPUT_FORCE.into(),
                source: Some(collider.source),
                resource: ResourceTypeId::new(RESOURCE_OUTPUTS),
                word: index as u32 * FORCE_RECORD_WORDS as u32,
                components: 3,
                event: (threshold > 0.0).then(|| aestra_runtime::OutputEvent {
                    kind: EVENT_IMPACT.into(),
                    threshold,
                }),
            })
        })
        .collect()
}

/// Words per collider in the outputs (`forces.wgsl`'s `FORCE_RECORD`): force xyz, then its size.
const FORCE_RECORD_WORDS: u64 = 4;

/// The passes measuring the colliders' forces after a projection (fluid F11).
fn force_passes(grid: Grid) -> Vec<ExecutionOp> {
    vec![
        grid.pass(
            "measure_collider_forces",
            vec![
                ResourceAccess::read(RESOURCE_PRESSURE),
                ResourceAccess::read(RESOURCE_SOLID),
                ResourceAccess::write(RESOURCE_COLLIDER_FORCES),
                ResourceAccess::read(AESTRA_RESOURCE_STAGE_CONSTANTS),
                ResourceAccess::read(AESTRA_RESOURCE_FRAME),
                ResourceAccess::read(AESTRA_RESOURCE_HOST_BINDINGS),
            ],
        ),
        grid.single(
            "collider_force_total",
            vec![
                ResourceAccess::read(RESOURCE_COLLIDER_FORCES),
                ResourceAccess::read_write(RESOURCE_OUTPUTS),
                ResourceAccess::read(AESTRA_RESOURCE_STAGE_CONSTANTS),
            ],
        ),
    ]
}

/// The emission list and its ranks (fluid F10), at bindings 36 and 37.
fn emission_resources(
    resources: &mut Vec<ResourceDescriptor>,
    capacity: u32,
    groups: u64,
) -> aestra_runtime::EmissionLayout {
    pad_resources(resources, 36);
    let layout = aestra_runtime::EmissionLayout {
        resource: ResourceTypeId::new(RESOURCE_EMISSION),
        capacity,
    };
    resources.extend([
        ResourceDescriptor {
            id: layout.resource.clone(),
            bytes: layout.bytes(),
            lifetime: ResourceLifetime::Transient,
        },
        ResourceDescriptor {
            id: ResourceTypeId::new(RESOURCE_EMISSION_SCRATCH),
            bytes: (u64::from(EMISSION_SCRATCH_HEADER) + groups * 65) * 4,
            lifetime: ResourceLifetime::Transient,
        },
    ]);
    layout
}

/// Words before the emission ranks (`emit.wgsl`'s `EMISSION_SCRATCH_HEADER`).
const EMISSION_SCRATCH_HEADER: u32 = 16;

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
                let sparse = payload.get_bool("sparse").unwrap_or(false);
                if sparse {
                    if !(MIN_RESOLUTION..=MAX_SPARSE_RESOLUTION).contains(&resolution)
                        || resolution % BRICK_EDGE != 0
                    {
                        return Err(format!(
                            "a sparse grid's resolution must be a multiple of {BRICK_EDGE} between \
                             {MIN_RESOLUTION} and {MAX_SPARSE_RESOLUTION}, got {resolution}"
                        ));
                    }
                    let budget = count(payload, "brick_budget")?;
                    if !(1..=MAX_BRICK_BUDGET).contains(&budget) {
                        return Err(format!(
                            "the brick budget must be between 1 and {MAX_BRICK_BUDGET}, got {budget}"
                        ));
                    }
                    scalar(payload, "brick_threshold")?;
                    if payload.get_bool("flow_map").unwrap_or(false) {
                        return Err("flow maps do not run on a sparse grid".into());
                    }
                } else if !(MIN_RESOLUTION..=MAX_RESOLUTION).contains(&resolution)
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
                for name in [
                    "density_dissipation",
                    "velocity_dissipation",
                    "pressure_tolerance",
                ] {
                    if scalar(payload, name)? < 0.0 {
                        return Err(format!("'{name}' must not be negative"));
                    }
                }
                if payload.get_bool("flow_map").unwrap_or(false) {
                    let cycle = count(payload, "flow_map_cycle")?;
                    if !(1..=MAX_FLOW_MAP_CYCLE).contains(&cycle) {
                        return Err(format!(
                            "the flow map cycle must be between 1 and {MAX_FLOW_MAP_CYCLE} steps, \
                             got {cycle}"
                        ));
                    }
                    if resolution > MAX_FLOW_MAP_RESOLUTION {
                        return Err(format!(
                            "flow maps run on grids up to {MAX_FLOW_MAP_RESOLUTION} cells a side, \
                             got {resolution}"
                        ));
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
                if scalar(payload, "duration")? < 0.0 {
                    return Err("a source's duration must not be negative".into());
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
                pack_volume(payload, &QualityTier::high())?;
                VOLUME_ENTRY
            }
            MODULE_WORLD_COLLIDER => {
                if !scalar(payload, "offset")?.is_finite() {
                    return Err("the world collider's offset must be finite".into());
                }
                "mark_world_solids"
            }
            MODULE_SECONDARY_EMISSION => {
                pack_emission(
                    &ExtensionModulePlan {
                        source: module.id,
                        module_type: module.module_type.clone(),
                        entry_point: String::new(),
                        parameters: payload.clone(),
                        host_fields: Default::default(),
                    },
                    1,
                    &TierScale::of(&QualityTier::high(), MIN_RESOLUTION, false),
                    &QualityTier::high(),
                )?;
                "emit_offsets"
            }
            liquid @ (MODULE_LIQUID_GRID | MODULE_LIQUID_BLOCK | MODULE_LIQUID_SOURCE
            | MODULE_LIQUID_LOOK) => {
                liquid::validate_liquid_module(liquid, payload)?;
                match liquid {
                    MODULE_LIQUID_GRID => "liquid_mark",
                    MODULE_LIQUID_LOOK => LIQUID_LOOK_ENTRY,
                    _ => "liquid_emit",
                }
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
/// only with combustion, last, so the others keep their bindings. The multigrid pressure solve's and a
/// sparse grid's bricks are always declared, sized only when they are used.
fn resources(
    grid: Grid,
    constant_words: usize,
    fire: bool,
    multigrid: bool,
    flow_map_cycle: u32,
) -> Vec<ResourceDescriptor> {
    let cells = grid.cells();
    let field = |id: &str, bytes_per_cell: u64, lifetime| ResourceDescriptor {
        id: ResourceTypeId::new(id),
        bytes: cells * bytes_per_cell,
        lifetime,
    };
    let mut resources = vec![
        field(RESOURCE_VELOCITY, 16, ResourceLifetime::Persistent),
        field(RESOURCE_DENSITY, 4, ResourceLifetime::Persistent),
        field(RESOURCE_VELOCITY_NEXT, 16, ResourceLifetime::Transient),
        field(RESOURCE_DENSITY_NEXT, 4, ResourceLifetime::Transient),
        // Persistent (fluid F5): each tick's solve starts from the last one's pressure.
        field(RESOURCE_PRESSURE, 4, ResourceLifetime::Persistent),
        field(RESOURCE_PRESSURE_NEXT, 4, ResourceLifetime::Transient),
        field(RESOURCE_DIVERGENCE, 4, ResourceLifetime::Transient),
        field(RESOURCE_VORTICITY, 16, ResourceLifetime::Transient),
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
    resources.push(field(
        RESOURCE_VELOCITY_HAT,
        16,
        ResourceLifetime::Transient,
    ));
    resources.push(field(RESOURCE_SCALAR_HAT, 4, ResourceLifetime::Transient));
    resources.push(field(RESOURCE_SOLID, 16, ResourceLifetime::Transient));
    let (fine_cells, level_cells) = if multigrid {
        (cells, grid.level_cells())
    } else {
        (1, 1)
    };
    // The partial sums serve the pressure solve and the flow maps' safeguard.
    let groups = if multigrid || flow_map_cycle > 0 {
        grid.groups()
    } else {
        1
    };
    let scratch = |id: &str, bytes: u64| ResourceDescriptor {
        id: ResourceTypeId::new(id),
        // Never empty: a zero-sized resource cannot be bound.
        bytes: bytes.max(16),
        lifetime: ResourceLifetime::Transient,
    };
    resources.extend([
        scratch(RESOURCE_PCG_VECTORS, fine_cells * 8),
        scratch(RESOURCE_MG_SOLUTION, level_cells * 4),
        scratch(RESOURCE_MG_RHS, level_cells * 4),
        scratch(RESOURCE_MG_FLAGS, level_cells * 4),
        // Two vec4 of scalars, then one partial sum per fine workgroup.
        scratch(RESOURCE_PCG_REDUCTION, (2 + groups) * 16),
    ]);
    // Flow maps (fluid F6): per cell, 3 floats per stored step, a velocity, 18 floats of forward map
    // (all state); 18 of backward map, the forces and the impulse (scratch). A few bytes without.
    let mapped = if flow_map_cycle > 0 { cells } else { 0 };
    let map_state = |id: &str, bytes: u64, lifetime| ResourceDescriptor {
        id: ResourceTypeId::new(id),
        bytes: bytes.max(16),
        lifetime,
    };
    resources.extend([
        map_state(
            RESOURCE_LFM_HISTORY,
            mapped * 12 * u64::from(flow_map_cycle),
            ResourceLifetime::Persistent,
        ),
        map_state(
            RESOURCE_LFM_INITIAL,
            mapped * 16,
            ResourceLifetime::Persistent,
        ),
        map_state(
            RESOURCE_LFM_FORWARD,
            mapped * 72,
            ResourceLifetime::Persistent,
        ),
        map_state(
            RESOURCE_LFM_BACKWARD,
            mapped * 72,
            ResourceLifetime::Transient,
        ),
        map_state(RESOURCE_LFM_FORCE, mapped * 16, ResourceLifetime::Transient),
        map_state(
            RESOURCE_LFM_IMPULSE,
            mapped * 16,
            ResourceLifetime::Transient,
        ),
    ]);
    // A sparse grid's bricks (fluid F7): the header, list, slots' bricks and brick table (state); the
    // allocation's flags, free list and new bricks' ranks; the passes' workgroup counts (state, so a
    // tick's first passes cover the bricks the last one left). A dense grid's are scratch: its
    // checkpoints carry nothing for them.
    let brick_state = if grid.slots.is_some() {
        ResourceLifetime::Persistent
    } else {
        ResourceLifetime::Transient
    };
    let (bricks, brick_scratch) = match grid.slots {
        Some(slots) => {
            let (slots, table) = (u64::from(slots), grid.brick_table());
            (
                (16 + 2 * slots + table) * 4,
                (16 + 4 * slots + 2 * table + table.div_ceil(64)) * 4,
            )
        }
        None => (0, 0),
    };
    resources.extend([
        map_state(RESOURCE_BRICKS, bricks, brick_state),
        map_state(
            RESOURCE_BRICK_SCRATCH,
            brick_scratch,
            ResourceLifetime::Transient,
        ),
        map_state(
            RESOURCE_BRICK_DISPATCH,
            if grid.slots.is_some() { 64 } else { 0 },
            brick_state,
        ),
    ]);
    if fire {
        resources.extend([
            field(RESOURCE_TEMPERATURE, 4, ResourceLifetime::Persistent),
            field(RESOURCE_TEMPERATURE_NEXT, 4, ResourceLifetime::Transient),
            field(RESOURCE_FUEL, 4, ResourceLifetime::Persistent),
            field(RESOURCE_FUEL_NEXT, 4, ResourceLifetime::Transient),
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
    grid: Grid,
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
    /// The pressure is solved by MGPCG to `tolerance` (fluid F5), not by Jacobi sweeps.
    multigrid: bool,
    tolerance: f32,
    /// Steps in a flow-map cycle (fluid F6); 0 without flow maps.
    flow_map_cycle: u32,
    /// A Secondary Emission's list capacity (fluid F10), when the stage has one.
    emission: Option<u32>,
    /// A World Collider is present (fluid F11): solids are also marked from the host's world SDF.
    world: bool,
}

/// Packs the stage constants `solver.wgsl` reads.
fn pack_constants(
    modules: &[ExtensionModulePlan],
    tier: &QualityTier,
) -> Result<PackedStage, String> {
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

    // The quality tier (fluid F12) coarsens the grid over the same box and caps the solve lower.
    let sparse = grid.parameters.get_bool("sparse").unwrap_or(false);
    let scaled = TierScale::of(tier, count(&grid.parameters, "resolution")?, sparse);
    let resolution = scaled.resolution;
    let iterations = QualityTier::scale_count(
        count(&grid.parameters, "pressure_iterations")?,
        tier.iterations,
        1,
        1,
    );
    let cell_size = scalar(&grid.parameters, "cell_size")? * scaled.cell_growth;
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
    // Flow maps (fluid F6): the cycle's steps, 0 without them.
    if grid.parameters.get_bool("flow_map").unwrap_or(false) {
        words[18] = count(&grid.parameters, "flow_map_cycle")?;
    }
    // MacCormack advection (fluid F4); off falls back to plain semi-Lagrangian.
    words[11] = u32::from(grid.parameters.get_bool("sharp_advection").unwrap_or(true));
    // A sparse grid (fluid F7): its slots — the budget and the empty slot 0 — and the threshold that
    // keeps a brick active; 0 slots for a dense grid.
    let slots = if sparse {
        let slots = scaled.bricks(count(&grid.parameters, "brick_budget")?) + 1;
        words[19] = slots;
        words[20] = scalar(&grid.parameters, "brick_threshold")?.to_bits();
        Some(slots)
    } else {
        None
    };
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
        words[base + 16] = scalar(&source.parameters, "duration")?.to_bits();
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
    let world = pack_world(modules, &mut words)?;
    let grid_of = Grid {
        resolution,
        slots,
        liquid: false,
    };
    // A Secondary Emission's block (fluid F10) goes last, header word 23 naming its first word.
    let emission = match emission_module(modules)? {
        Some(module) => {
            let block = pack_emission(module, grid_of.groups(), &scaled, tier)?;
            words[23] = words.len() as u32;
            words.extend(block);
            Some(block[3])
        }
        None => None,
    };
    Ok(PackedStage {
        grid: grid_of,
        emission,
        world,
        iterations,
        cell_size,
        origin,
        sharp: words[11] != 0,
        colliders: !colliders.is_empty(),
        multigrid: grid
            .parameters
            .get_bool("multigrid_pressure")
            .unwrap_or(true),
        tolerance: scalar(&grid.parameters, "pressure_tolerance")?,
        flow_map_cycle: words[18],
        constants: words,
        fire: combustion.is_some(),
    })
}

/// Packs a Volume Look's inputs into the constant words `volume.wgsl` reads.
fn pack_volume(payload: &PropertyBag, tier: &QualityTier) -> Result<Vec<u32>, String> {
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
    // A quality tier (fluid F12) marches fewer steps (the look integrates over distance, so it holds).
    words[0] = QualityTier::scale_count(steps, tier.presentation, 1, steps.min(8));
    words[1] = if shadow_steps == 0 {
        0
    } else {
        QualityTier::scale_count(shadow_steps, tier.presentation, 1, 1)
    };
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

/// How a stage's grid is stored and how its passes cover it (fluid F7).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Grid {
    resolution: u32,
    /// A sparse grid's brick slots — its budget, and slot 0, which stays empty; `None` when dense.
    slots: Option<u32>,
    /// A liquid's grid (fluid F8): its passes are the liquid program's.
    liquid: bool,
}

/// Multigrid levels a sparse grid's bricks hold: 8³, 4³, 2³ and 1 cell.
const SPARSE_LEVELS: usize = 4;

impl Grid {
    fn program(self) -> &'static str {
        if self.liquid {
            PROGRAM_LIQUID
        } else if self.slots.is_some() {
            PROGRAM_SOLVER_SPARSE
        } else {
            PROGRAM_SOLVER
        }
    }

    /// Cells every grid resource stores.
    fn cells(self) -> u64 {
        match self.slots {
            Some(slots) => u64::from(slots) * u64::from(BRICK_EDGE.pow(3)),
            None => u64::from(self.resolution).pow(3),
        }
    }

    /// Entries in a sparse grid's brick table.
    fn brick_table(self) -> u64 {
        u64::from(self.resolution / BRICK_EDGE).pow(3)
    }

    /// How a sparse grid's fields are stored, for their readers (`grid_sparse.wgsl`'s `bricks`: the
    /// header, the list, each slot's brick, then the table).
    fn brick_layout(self) -> Option<aestra_runtime::BrickLayout> {
        self.slots.map(|slots| aestra_runtime::BrickLayout {
            edge: BRICK_EDGE,
            slots,
            table: ResourceTypeId::new(RESOURCE_BRICKS),
            table_word: 16 + 2 * slots,
            slot_bricks_word: 16 + slots,
        })
    }

    /// The multigrid levels' resolutions, finest first.
    fn levels(self) -> Vec<u32> {
        match self.slots {
            Some(_) => (0..SPARSE_LEVELS)
                .map(|level| self.resolution >> level)
                .collect(),
            None => multigrid_levels(self.resolution),
        }
    }

    /// Cells every multigrid level stores together.
    fn level_cells(self) -> u64 {
        match self.slots {
            Some(slots) => u64::from(slots) * (512 + 64 + 8 + 1),
            None => self.levels().iter().map(|n| u64::from(*n).pow(3)).sum(),
        }
    }

    /// Workgroups a fine-level pass runs at most: one partial sum each.
    fn groups(self) -> u64 {
        match self.slots {
            Some(slots) => u64::from(slots) * 8,
            None => u64::from(self.resolution / WORKGROUP).pow(3),
        }
    }

    /// A pass over the cells of multigrid level `level` (0: the grid itself) in 4³ workgroups: the
    /// whole level on a dense grid; on a sparse one, the active bricks — counts the allocation writes,
    /// within the bound of every slot.
    fn cover(self, level: usize) -> (StagedDispatch, Option<IndirectDispatch>) {
        match self.slots {
            None => {
                let groups = self.levels()[level].div_ceil(WORKGROUP);
                (
                    StagedDispatch {
                        x: groups,
                        y: groups,
                        z: groups,
                    },
                    None,
                )
            }
            Some(slots) => {
                let per_brick = BRICK_EDGE.pow(3) >> (3 * level);
                (
                    StagedDispatch {
                        x: (slots * per_brick).div_ceil(64),
                        y: 1,
                        z: 1,
                    },
                    Some(IndirectDispatch {
                        resource: ResourceTypeId::new(RESOURCE_BRICK_DISPATCH),
                        word: 4 * level as u32,
                    }),
                )
            }
        }
    }

    /// One pass of the solver program, run as `dispatch` (and `indirect`). On a sparse grid every
    /// solver entry finds its cells through the bricks.
    fn op(
        self,
        entry: &str,
        mut accesses: Vec<ResourceAccess>,
        (dispatch, indirect): (StagedDispatch, Option<IndirectDispatch>),
    ) -> ExecutionOp {
        let bricks = |access: &ResourceAccess| access.resource.as_str() == RESOURCE_BRICKS;
        if self.slots.is_some() && !accesses.iter().any(bricks) {
            accesses.push(ResourceAccess::read(RESOURCE_BRICKS));
        }
        ExecutionOp::Compute(ComputeOp {
            name: format!("fluid/{entry}"),
            program: Some(ComputeProgramId::new(self.program())),
            entry_point: entry.into(),
            accesses,
            dispatch,
            indirect,
        })
    }

    /// A pass over the grid's cells.
    fn pass(self, entry: &str, accesses: Vec<ResourceAccess>) -> ExecutionOp {
        self.op(entry, accesses, self.cover(0))
    }

    /// A single-workgroup pass.
    fn single(self, entry: &str, accesses: Vec<ResourceAccess>) -> ExecutionOp {
        self.op(entry, accesses, (StagedDispatch { x: 1, y: 1, z: 1 }, None))
    }

    /// A sparse grid's allocation (`grid_sparse.wgsl`), first in every tick: the bricks the fluid
    /// needs are given slots, and those it left are freed.
    fn allocation(self, fire: bool) -> Vec<ExecutionOp> {
        use ResourceAccess as Access;
        let constants = || Access::read(AESTRA_RESOURCE_STAGE_CONSTANTS);
        let table = StagedDispatch {
            x: self.brick_table().div_ceil(64) as u32,
            y: 1,
            z: 1,
        };
        let mut activity = vec![
            Access::read(RESOURCE_BRICKS),
            Access::write(RESOURCE_BRICK_SCRATCH),
            Access::read(RESOURCE_DENSITY),
            constants(),
        ];
        let mut zero = vec![
            Access::read(RESOURCE_BRICKS),
            Access::read(RESOURCE_BRICK_SCRATCH),
            Access::write(RESOURCE_VELOCITY),
            Access::write(RESOURCE_DENSITY),
            Access::write(RESOURCE_PRESSURE),
            constants(),
        ];
        if fire {
            activity.extend([
                Access::read(RESOURCE_TEMPERATURE),
                Access::read(RESOURCE_FUEL),
            ]);
            zero.extend([
                Access::write(RESOURCE_TEMPERATURE),
                Access::write(RESOURCE_FUEL),
            ]);
        }
        let suffix = if fire { "_fire" } else { "" };
        vec![
            // Last tick's bricks, one workgroup each.
            self.op(&format!("brick_activity{suffix}"), activity, self.cover(1)),
            self.op(
                "brick_need",
                vec![
                    Access::read(RESOURCE_BRICKS),
                    Access::read_write(RESOURCE_BRICK_SCRATCH),
                    constants(),
                    Access::read(AESTRA_RESOURCE_FRAME),
                    Access::read(AESTRA_RESOURCE_HOST_BINDINGS),
                ],
                (table, None),
            ),
            self.single(
                "brick_plan",
                vec![
                    Access::read(RESOURCE_BRICKS),
                    Access::read_write(RESOURCE_BRICK_SCRATCH),
                    constants(),
                ],
            ),
            self.op(
                "brick_assign",
                vec![
                    Access::read_write(RESOURCE_BRICKS),
                    Access::read_write(RESOURCE_BRICK_SCRATCH),
                    constants(),
                ],
                (table, None),
            ),
            self.single(
                "brick_compact",
                vec![
                    Access::read_write(RESOURCE_BRICKS),
                    Access::read(RESOURCE_BRICK_SCRATCH),
                    Access::write(RESOURCE_BRICK_DISPATCH),
                    constants(),
                ],
            ),
            // This tick's bricks.
            self.op(&format!("brick_zero{suffix}"), zero, self.cover(0)),
        ]
    }
}

/// The velocity carried one step without flow maps: semi-Lagrangian, and with MacCormack its
/// correction, then copied back.
fn advect_velocity(grid: Grid, sharp: bool) -> Vec<ExecutionOp> {
    let constants = || ResourceAccess::read(AESTRA_RESOURCE_STAGE_CONSTANTS);
    let frame = || ResourceAccess::read(AESTRA_RESOURCE_FRAME);
    let copy = |from: &str, to: &str| {
        ExecutionOp::Copy(CopyOp {
            from: ResourceTypeId::new(from),
            to: ResourceTypeId::new(to),
        })
    };
    let mut ops = vec![grid.pass(
        "advect_velocity",
        vec![
            ResourceAccess::read(RESOURCE_VELOCITY),
            ResourceAccess::write(RESOURCE_VELOCITY_NEXT),
            constants(),
            frame(),
        ],
    )];
    if sharp {
        // MacCormack: correct the semi-Lagrangian result, then take the corrected one.
        ops.push(grid.pass(
            "correct_velocity",
            vec![
                ResourceAccess::read(RESOURCE_VELOCITY),
                ResourceAccess::read(RESOURCE_VELOCITY_NEXT),
                ResourceAccess::write(RESOURCE_VELOCITY_HAT),
                constants(),
                frame(),
            ],
        ));
        ops.push(copy(RESOURCE_VELOCITY_HAT, RESOURCE_VELOCITY));
    } else {
        ops.push(copy(RESOURCE_VELOCITY_NEXT, RESOURCE_VELOCITY));
    }
    ops
}

/// Jacobi sweeps that ping-pong between the pressure grids: a pair per repeat, no copies; an odd
/// count ends with one sweep copied back.
fn jacobi_pressure(grid: Grid, iterations: u32) -> Vec<ExecutionOp> {
    let relax = |back: bool| {
        let (entry, from, to) = if back {
            (
                "relax_pressure_back",
                RESOURCE_PRESSURE_NEXT,
                RESOURCE_PRESSURE,
            )
        } else {
            ("relax_pressure", RESOURCE_PRESSURE, RESOURCE_PRESSURE_NEXT)
        };
        grid.pass(
            entry,
            vec![
                ResourceAccess::read(from),
                ResourceAccess::write(to),
                ResourceAccess::read(RESOURCE_DIVERGENCE),
                ResourceAccess::read(RESOURCE_SOLID),
                ResourceAccess::read(AESTRA_RESOURCE_STAGE_CONSTANTS),
            ],
        )
    };
    let mut ops = Vec::new();
    if iterations >= 2 {
        ops.push(ExecutionOp::Repeat {
            policy: RepeatPolicy::FixedCount(iterations / 2),
            // The barriers close each sweep: the next reads what it wrote.
            body: vec![
                relax(false),
                ExecutionOp::Barrier,
                relax(true),
                ExecutionOp::Barrier,
            ],
        });
    }
    if iterations % 2 == 1 {
        ops.push(relax(false));
        ops.push(ExecutionOp::Copy(CopyOp {
            from: ResourceTypeId::new(RESOURCE_PRESSURE_NEXT),
            to: ResourceTypeId::new(RESOURCE_PRESSURE),
        }));
    }
    ops
}

/// The MGPCG pressure solve (fluid F5, `pressure.wgsl`): with colliders, every level's solid flags;
/// the right-hand side; then conjugate-gradient iterations, each preconditioned by one V-cycle, in a
/// repeat that stops on the device once the relative residual is at most `tolerance` — at most
/// `iterations` times. No copies: the whole solve runs in one compute pass.
fn multigrid_pressure(
    grid: Grid,
    iterations: u32,
    tolerance: f32,
    colliders: bool,
    coarsen: bool,
) -> Vec<ExecutionOp> {
    use ResourceAccess as Access;
    let levels = grid.levels();
    let constants = || Access::read(AESTRA_RESOURCE_STAGE_CONSTANTS);
    // A pass's own accesses, and what finding a cell's neighbours reads: the solid flags and the
    // open sides.
    let with_walls = |mut accesses: Vec<ResourceAccess>| {
        accesses.extend([Access::read(RESOURCE_MG_FLAGS), constants()]);
        accesses
    };
    let level_pass = |pass: LevelPass, level: usize| {
        let accesses = match pass {
            LevelPass::SmoothRed | LevelPass::SmoothBlack | LevelPass::ProlongSmooth => {
                with_walls(vec![
                    Access::read_write(RESOURCE_MG_SOLUTION),
                    Access::read(RESOURCE_MG_RHS),
                ])
            }
            LevelPass::RestrictSmooth => with_walls(vec![
                Access::read_write(RESOURCE_MG_SOLUTION),
                Access::read_write(RESOURCE_MG_RHS),
            ]),
            // The fine flags from the solids; a coarse level's from the finer one's.
            LevelPass::Coarsen => vec![
                Access::read_write(RESOURCE_MG_FLAGS),
                Access::read(RESOURCE_SOLID),
                constants(),
            ],
        };
        grid.op(&pass.entry(level), accesses, grid.cover(level))
    };
    // A single-workgroup pass summing the partials into the scalars: a dense grid counts them from its
    // resolution, a sparse one from its active bricks — and the setup reads the open sides.
    let reduce = |entry: &str| {
        let mut accesses = vec![Access::read_write(RESOURCE_PCG_REDUCTION)];
        if grid.slots.is_none() || entry == "pcg_setup_finalize" {
            accesses.push(constants());
        }
        grid.single(entry, accesses)
    };
    // A pass over the fine grid. A liquid's operator reads its faces' pressure coefficients (fluid
    // F9, `face_coefficient`).
    let fine = |entry: &str, mut accesses: Vec<ResourceAccess>| {
        if grid.liquid && matches!(entry, "pcg_setup" | "pcg_apply") {
            accesses.push(Access::read(RESOURCE_VELOCITY_HAT));
        }
        grid.pass(entry, accesses)
    };

    let mut ops = Vec::new();
    if grid.liquid {
        // A liquid's fine flags come from its particles every step (`liquid_mark`); the coarse ones
        // follow them.
        for level in 1..levels.len() {
            ops.push(level_pass(LevelPass::Coarsen, level));
        }
    } else if colliders && coarsen {
        // The solid flags, once a tick (a second solve reuses them).
        for level in 0..levels.len() {
            ops.push(level_pass(LevelPass::Coarsen, level));
        }
    }
    ops.push(fine(
        "pcg_setup",
        with_walls(vec![
            Access::read(RESOURCE_DIVERGENCE),
            Access::read(RESOURCE_PRESSURE),
            Access::write(RESOURCE_MG_RHS),
            Access::write(RESOURCE_PCG_REDUCTION),
        ]),
    ));
    ops.push(reduce("pcg_setup_finalize"));
    // r loses b's mean (a closed domain), and the first V-cycle's red sweep.
    ops.push(fine(
        "pcg_start",
        with_walls(vec![
            Access::read_write(RESOURCE_MG_RHS),
            Access::read(RESOURCE_PCG_REDUCTION),
            Access::write(RESOURCE_MG_SOLUTION),
        ]),
    ));

    // One iteration: z = M⁻¹·r (a V-cycle, whose fine red sweep the previous pass took), β,
    // p = z + β·p with q = A·p, α, x and r step with the next V-cycle's red sweep, |r| / |b|. Every
    // level sweeps red, black on the way down and black, red on the way up — the coarsest red, black,
    // red — a palindrome, so the cycle is symmetric.
    let coarsest = levels.len() - 1;
    let mut body = vec![level_pass(LevelPass::SmoothBlack, 0)];
    for level in 1..=coarsest {
        body.push(level_pass(LevelPass::RestrictSmooth, level));
        body.push(level_pass(LevelPass::SmoothBlack, level));
    }
    body.push(level_pass(LevelPass::SmoothRed, coarsest));
    for level in (0..coarsest).rev() {
        body.push(level_pass(LevelPass::ProlongSmooth, level));
        if level > 0 {
            body.push(level_pass(LevelPass::SmoothRed, level));
        }
    }
    // The fine level's last red sweep, with the partials of r·z.
    body.push(fine(
        "pcg_smooth_dot",
        with_walls(vec![
            Access::read_write(RESOURCE_MG_SOLUTION),
            Access::read(RESOURCE_MG_RHS),
            Access::write(RESOURCE_PCG_REDUCTION),
        ]),
    ));
    body.push(reduce("pcg_beta"));
    body.push(fine(
        "pcg_apply",
        with_walls(vec![
            Access::read(RESOURCE_MG_SOLUTION),
            Access::read_write(RESOURCE_PCG_VECTORS),
            Access::read_write(RESOURCE_PCG_REDUCTION),
        ]),
    ));
    body.push(reduce("pcg_alpha"));
    body.push(fine(
        "pcg_step",
        with_walls(vec![
            Access::read_write(RESOURCE_PRESSURE),
            Access::read(RESOURCE_PCG_VECTORS),
            Access::read_write(RESOURCE_MG_RHS),
            Access::read_write(RESOURCE_PCG_REDUCTION),
            Access::write(RESOURCE_MG_SOLUTION),
        ]),
    ));
    body.push(reduce("pcg_residual"));
    ops.push(ExecutionOp::Repeat {
        policy: RepeatPolicy::UntilConverged {
            residual: ResourceTypeId::new(RESOURCE_PCG_REDUCTION),
            tolerance,
            max: iterations,
            test_first: true,
        },
        body: with_barriers(body),
    });
    ops
}
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
                let mut constants = pack_volume(&look.parameters, input.tier)?;
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
            grid,
            iterations,
            cell_size,
            origin,
            constants,
            fire,
            sharp,
            colliders,
            multigrid,
            tolerance,
            flow_map_cycle,
            emission,
            world,
        } = pack_constants(input.modules, input.tier)?;
        let flow_map = flow_map_cycle > 0;
        let resolution = grid.resolution;
        let pass = |entry: &str, accesses: Vec<ResourceAccess>| grid.pass(entry, accesses);
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
        if grid.slots.is_some() {
            // A sparse grid first sets which bricks this tick simulates.
            steps.extend(grid.allocation(fire));
        }
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
        if world {
            // The host's scene (fluid F11), after the analytic colliders.
            steps.push(world_solids(grid));
        }
        if flow_map {
            // The velocity before the force passes: what they add is the flow maps' force field.
            steps.push(copy(RESOURCE_VELOCITY, RESOURCE_LFM_FORCE));
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
        if flow_map {
            // The forces, then the leapfrog-advected midpoint velocity (fluid F6).
            steps.push(pass(
                "lfm_forces",
                vec![
                    read(RESOURCE_VELOCITY),
                    read_write(RESOURCE_LFM_FORCE),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(pass(
                "lfm_advect",
                vec![
                    read(RESOURCE_LFM_INITIAL),
                    read(RESOURCE_LFM_HISTORY),
                    read(RESOURCE_LFM_FORCE),
                    write(RESOURCE_VELOCITY_NEXT),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(copy(RESOURCE_VELOCITY_NEXT, RESOURCE_VELOCITY));
        } else {
            steps.extend(advect_velocity(grid, sharp));
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
        let solve = |coarsen: bool| {
            if multigrid {
                multigrid_pressure(grid, iterations, tolerance, colliders || world, coarsen)
            } else {
                jacobi_pressure(grid, iterations)
            }
        };
        steps.extend(solve(true));
        steps.push(pass(
            "project",
            vec![
                read_write(RESOURCE_VELOCITY),
                read(RESOURCE_PRESSURE),
                read(RESOURCE_SOLID),
                constants_read(),
            ],
        ));
        if colliders {
            // The pressure's push on each collider, for the host (fluid F11).
            steps.extend(force_passes(grid));
        }
        if flow_map {
            // The projected midpoint velocity is stored and the forward maps marched through it.
            steps.push(pass(
                "lfm_march_forward",
                vec![
                    read(RESOURCE_VELOCITY),
                    read_write(RESOURCE_LFM_FORWARD),
                    read(RESOURCE_LFM_FORCE),
                    read_write(RESOURCE_LFM_INITIAL),
                    write(RESOURCE_LFM_HISTORY),
                    constants_read(),
                    frame_read(),
                ],
            ));
        }
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
        if flow_map {
            // The cycle's last step (each pass does nothing on the others; the second solve starts
            // converged): the impulse mapped back along the backward maps, its round-trip error
            // measured and taken off, then projected into the next cycle's velocity.
            steps.push(pass(
                "lfm_pull_back",
                vec![
                    read(RESOURCE_VELOCITY),
                    read(RESOURCE_LFM_HISTORY),
                    write(RESOURCE_LFM_BACKWARD),
                    read(RESOURCE_LFM_INITIAL),
                    write(RESOURCE_LFM_IMPULSE),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(pass(
                "lfm_measure_error",
                vec![
                    read(RESOURCE_LFM_FORWARD),
                    read(RESOURCE_LFM_IMPULSE),
                    read(RESOURCE_LFM_INITIAL),
                    write(RESOURCE_LFM_FORCE),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(pass(
                "lfm_compensate",
                vec![
                    read(RESOURCE_LFM_BACKWARD),
                    read(RESOURCE_LFM_FORCE),
                    read(RESOURCE_LFM_IMPULSE),
                    write(RESOURCE_VELOCITY),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(pass(
                "lfm_impulse_divergence",
                vec![
                    read(RESOURCE_VELOCITY),
                    write(RESOURCE_DIVERGENCE),
                    read(RESOURCE_SOLID),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.extend(solve(false));
            steps.push(pass(
                "lfm_project",
                vec![
                    read_write(RESOURCE_VELOCITY),
                    read(RESOURCE_PRESSURE),
                    read(RESOURCE_SOLID),
                    constants_read(),
                    frame_read(),
                ],
            ));
            // The safeguard: the mapped velocity's energy against the last midpoint velocity's.
            steps.push(pass(
                "lfm_energy",
                vec![
                    read(RESOURCE_VELOCITY),
                    read(RESOURCE_LFM_HISTORY),
                    write(RESOURCE_PCG_REDUCTION),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(grid.single(
                "lfm_energy_total",
                vec![
                    read_write(RESOURCE_PCG_REDUCTION),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(pass(
                "lfm_restart",
                vec![
                    read_write(RESOURCE_VELOCITY),
                    read(RESOURCE_LFM_HISTORY),
                    read(RESOURCE_PCG_REDUCTION),
                    write(RESOURCE_LFM_INITIAL),
                    constants_read(),
                    frame_read(),
                ],
            ));
        }

        let mut resources = resources(grid, constants.len(), fire, multigrid, flow_map_cycle);
        let mut emissions = Vec::new();
        if let Some(capacity) = emission {
            // Last, the cells the fluid asks particles of (fluid F10): masked and ranked, the ranks
            // turned to offsets, the records written in cell order.
            let (mask, value) = if fire {
                ("emit_cells_fire", RESOURCE_TEMPERATURE)
            } else {
                ("emit_cells", RESOURCE_DENSITY)
            };
            steps.push(pass(
                mask,
                vec![
                    read(RESOURCE_VELOCITY),
                    read(value),
                    write(RESOURCE_EMISSION_SCRATCH),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.extend(emission_ranking(grid));
            steps.push(pass(
                "emit_cells_write",
                vec![
                    read(RESOURCE_VELOCITY),
                    read(RESOURCE_EMISSION_SCRATCH),
                    write(RESOURCE_EMISSION),
                    constants_read(),
                    frame_read(),
                ],
            ));
            emissions.push(emission_resources(&mut resources, capacity, grid.groups()));
        }
        if world {
            world_resource(&mut resources);
        }
        let mut outputs = Vec::new();
        if colliders {
            let modules: Vec<&ExtensionModulePlan> = input
                .modules
                .iter()
                .filter(|module| is_collider(&module.module_type.0))
                .collect();
            outputs = force_resources(&mut resources, &modules, grid.groups())?;
        }

        Ok(ExecutionBlock {
            resources,
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
                    bricks: grid.brick_layout(),
                })
                .collect(),
            emissions,
            outputs,
        })
    }
}

/// The pass marking the cells inside the host's world SDF solid (fluid F11).
fn world_solids(grid: Grid) -> ExecutionOp {
    grid.pass(
        "mark_world_solids",
        vec![
            ResourceAccess::write(RESOURCE_SOLID),
            ResourceAccess::write(RESOURCE_DENSITY),
            ResourceAccess::read(AESTRA_RESOURCE_STAGE_CONSTANTS),
            ResourceAccess::read(AESTRA_RESOURCE_FRAME),
            ResourceAccess::read(AESTRA_RESOURCE_WORLD_SDF),
        ],
    )
}

/// The one-workgroup pass turning a secondary emission's workgroup totals into offsets (fluid F10).
/// It finds no cell, so even on a sparse grid it reads no bricks.
fn emission_ranking(grid: Grid) -> Vec<ExecutionOp> {
    vec![ExecutionOp::Compute(ComputeOp {
        name: "fluid/emit_offsets".into(),
        program: Some(ComputeProgramId::new(grid.program())),
        entry_point: "emit_offsets".into(),
        accesses: vec![
            ResourceAccess::read_write(RESOURCE_EMISSION_SCRATCH),
            ResourceAccess::write(RESOURCE_EMISSION),
            ResourceAccess::read(AESTRA_RESOURCE_STAGE_CONSTANTS),
        ],
        dispatch: StagedDispatch { x: 1, y: 1, z: 1 },
        indirect: None,
    })]
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
