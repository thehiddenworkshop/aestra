//! Free-surface liquids (fluid F8): the *Liquid Solver* stage, its modules and its lowering.
//!
//! A liquid is APIC particles on the solver's MAC grid (`liquid.wgsl`). The particles belong to the
//! stage — a persistent resource, like the gas's grids — rather than to an emitter: they carry what
//! APIC needs (position, velocity, the affine matrix C) and nothing an emitter draws. Each tick, one
//! planning pass sizes the emission, then every substep transfers the particles to the grid, makes
//! the flow incompressible with the gas's multigrid pressure solve (air cells at p = 0), and moves the
//! particles with the result. The grid's velocity and liquid fraction are declared as fields, so the
//! debug slices and volume presentations read them like a gas's.
//!
//! Modules: *Liquid Grid* (one: the box, the particle budget, substeps, gravity, the pressure solve),
//! *Liquid Block* (a box filled with liquid at the start), *Liquid Source* (a sphere pouring liquid,
//! host-bindable like a Density Source), and the fluid's colliders.

use super::*;

pub const CAPABILITY_LIQUID: &str = "org.example.aestra-fluid::capability/liquid";
pub const STAGE_LIQUID_SOLVER: &str = "org.example.aestra-fluid::stage/liquid_solver";
pub const MODULE_LIQUID_GRID: &str = "org.example.aestra-fluid::module/liquid_grid";
pub const MODULE_LIQUID_BLOCK: &str = "org.example.aestra-fluid::module/liquid_block";
pub const MODULE_LIQUID_SOURCE: &str = "org.example.aestra-fluid::module/liquid_source";
pub const MODULE_LIQUID_LOOK: &str = "org.example.aestra-fluid::module/liquid_look";
pub const PROGRAM_LIQUID: &str = "org.example.aestra-fluid::program/liquid";
/// The liquid look's march function (`liquid_look.wgsl`), composed after the volume interface.
pub const PROGRAM_LIQUID_LOOK: &str = "org.example.aestra-fluid::program/liquid_look";
pub const LIQUID_LOOK_ENTRY: &str = "liquid_look";
pub const LIQUID_LOOK_WGSL: &str = include_str!("liquid_look.wgsl");
/// The liquid's particles: per particle 5 `vec4`s — position (w: 1 when the slot holds one),
/// velocity, and the rows of the APIC matrix C.
pub const RESOURCE_LIQUID_PARTICLES: &str = "org.example.aestra-fluid::resource/liquid_particles";
/// The particle count and the first slot this tick emits into.
pub const RESOURCE_LIQUID_HEADER: &str = "org.example.aestra-fluid::resource/liquid_header";
/// The emission's and the particle passes' workgroup counts.
pub const RESOURCE_LIQUID_DISPATCH: &str = "org.example.aestra-fluid::resource/liquid_dispatch";
/// The particle-to-grid sums, in fixed point.
pub const RESOURCE_LIQUID_TRANSFER: &str = "org.example.aestra-fluid::resource/liquid_transfer";

/// The liquid's WGSL, after the solver, the dense grid and the pressure solve.
pub const LIQUID_WGSL: &str = include_str!("liquid.wgsl");

pub const MAX_LIQUID_PARTICLES: u32 = 1 << 20;
pub const MAX_LIQUID_SUBSTEPS: u32 = 4;
pub const MAX_LIQUID_BLOCKS: usize = 8;
/// Words one particle takes, and one block record.
const PARTICLE_BYTES: u64 = 5 * 16;
const BLOCK_WORDS: usize = 12;

/// The liquid program: the solver's shared kernels (divergence, projection, colliders), the dense
/// grid, the pressure solve with its per-level entry points, and the liquid's own passes.
pub fn liquid_program_wgsl() -> String {
    format!(
        "{SOLVER_WGSL}\n{GRID_DENSE_WGSL}\n{PRESSURE_WGSL}\n{FLOWMAP_WGSL}\n{LIQUID_WGSL}\n{}\n{}\n{}",
        multigrid_entries_wgsl(),
        aestra_gpu::HOST_BINDINGS_WGSL,
        aestra_gpu::reduce::REDUCE_WGSL
    )
}

/// The liquid's own entry points.
pub const LIQUID_ENTRY_POINTS: [&str; 7] = [
    "liquid_plan",
    "liquid_emit",
    "liquid_clear",
    "liquid_p2g",
    "liquid_faces",
    "liquid_mark",
    "liquid_g2p",
];

/// Every entry point of the liquid program: the solver's and the liquid's.
pub fn liquid_entry_points() -> Vec<String> {
    let mut entries = entry_points();
    entries.extend(LIQUID_ENTRY_POINTS.iter().map(|entry| entry.to_string()));
    entries
}

/// The resource types the liquid adds, as `(id, display name, lifetime)`.
pub(super) const LIQUID_RESOURCES: [(&str, &str, ResourceLifetime); 4] = [
    (
        RESOURCE_LIQUID_PARTICLES,
        "Liquid Particles",
        ResourceLifetime::Persistent,
    ),
    (
        RESOURCE_LIQUID_HEADER,
        "Liquid Particle Count",
        ResourceLifetime::Persistent,
    ),
    (
        RESOURCE_LIQUID_DISPATCH,
        "Liquid Workgroups",
        ResourceLifetime::Transient,
    ),
    (
        RESOURCE_LIQUID_TRANSFER,
        "Liquid Transfer",
        ResourceLifetime::Transient,
    ),
];

pub(super) fn liquid_grid_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    fluid_module(
        MODULE_LIQUID_GRID,
        "Liquid Grid",
        "The liquid's box: its grid, how many particles it may hold, substeps and gravity.",
        requires,
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![
        InputMetadata::new(
            "resolution",
            "Resolution",
            "Cells per side of the grid; a multiple of 4.",
            Value::U32(32),
            number(4.0, MIN_RESOLUTION as f32, Some(MAX_RESOLUTION as f32)),
        ),
        InputMetadata::new(
            "cell_size",
            "Cell Size",
            "Edge length of one grid cell. A block seeds 8 particles a cell.",
            Value::Scalar(3.0),
            number(0.1, 0.001, None),
        )
        .with_unit("units"),
        InputMetadata::new(
            "center",
            "Center",
            "Where the grid's centre sits. The box is closed on every side.",
            Value::Vec3([0.0, 48.0, 0.0]),
            vector(),
        )
        .with_unit("units"),
        InputMetadata::new(
            "particle_budget",
            "Particle Budget",
            "The most particles the liquid holds (80 bytes each); emission stops when it is full.",
            Value::U32(131_072),
            number(1024.0, 1.0, Some(MAX_LIQUID_PARTICLES as f32)),
        ),
        InputMetadata::new(
            "substeps",
            "Substeps",
            "Steps per tick: more follow fast liquid more closely, at a proportional cost. With \
             Spatiotemporal on, one is usually enough.",
            Value::U32(1),
            number(1.0, 1.0, Some(MAX_LIQUID_SUBSTEPS as f32)),
        ),
        InputMetadata::new(
            "spatiotemporal",
            "Spatiotemporal",
            "Spatiotemporal FLIP (Braun et al. 2026): each particle samples its own instant within the \
             step and the grid holds the step's average, so large steps — few substeps — keep a \
             smooth surface instead of rippling. The pressure then weighs each face by how full it \
             is. On a violent dam break at one substep a tick it follows the four-substep result \
             about as closely as plain FLIP does at two, for ~40% less time.",
            Value::Bool(true),
            InputControl::Toggle,
        ),
        InputMetadata::new(
            "gravity",
            "Gravity",
            "Acceleration of the liquid.",
            Value::Vec3([0.0, -400.0, 0.0]),
            vector(),
        )
        .with_unit("units/s²"),
        InputMetadata::new(
            "pressure_iterations",
            "Pressure Iterations",
            "The most iterations a step's pressure solve may take before it stops short of the \
             tolerance.",
            Value::U32(24),
            number(1.0, 1.0, Some(MAX_PRESSURE_ITERATIONS as f32)),
        ),
        InputMetadata::new(
            "pressure_tolerance",
            "Pressure Tolerance",
            "How small the solve's remaining error must be, relative to where it started, for it to \
             stop.",
            Value::Scalar(1e-3),
            number(0.0001, 0.0, Some(1.0)),
        ),
    ])
    .with_cost(8)
}

pub(super) fn liquid_block_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    fluid_module(
        MODULE_LIQUID_BLOCK,
        "Liquid Block",
        "A box of liquid at the start: 8 particles a cell, jittered.",
        requires,
    )
    .with_inputs(vec![
        InputMetadata::new(
            "center",
            "Center",
            "Centre of the box.",
            Value::Vec3([-28.0, 20.0, 0.0]),
            vector(),
        )
        .with_unit("units")
        .with_position_handle(),
        InputMetadata::new(
            "size",
            "Size",
            "The box's extent along each axis.",
            Value::Vec3([36.0, 36.0, 88.0]),
            vector(),
        )
        .with_unit("units"),
        InputMetadata::new(
            "velocity",
            "Velocity",
            "The liquid's velocity at the start.",
            Value::Vec3([0.0, 0.0, 0.0]),
            vector(),
        )
        .with_unit("units/s"),
    ])
    .with_cost(1)
}

pub(super) fn liquid_source_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    let bindable = vec![InputSourceKind::Constant, InputSourceKind::HostBinding];
    fluid_module(
        MODULE_LIQUID_SOURCE,
        "Liquid Source",
        "Pours liquid: particles emitted inside a sphere at a velocity.",
        requires,
    )
    .with_inputs(vec![
        InputMetadata::new(
            "position",
            "Position",
            "Centre of the source; a host object can drive it.",
            Value::Vec3([0.0, 70.0, 0.0]),
            vector(),
        )
        .with_unit("units")
        .with_sources(bindable.clone())
        .with_position_handle(),
        InputMetadata::new(
            "radius",
            "Radius",
            "Radius of the sphere the particles appear in.",
            Value::Scalar(6.0),
            number(0.5, 0.01, None),
        )
        .with_unit("units"),
        InputMetadata::new(
            "velocity",
            "Velocity",
            "The poured liquid's velocity; a host object's motion can drive it.",
            Value::Vec3([0.0, -40.0, 0.0]),
            vector(),
        )
        .with_unit("units/s")
        .with_sources(bindable),
        InputMetadata::new(
            "rate",
            "Rate",
            "Particles emitted per second.",
            Value::Scalar(2000.0),
            number(10.0, 0.0, None),
        )
        .with_unit("/s"),
    ])
    .with_cost(1)
}

pub(super) fn liquid_look_metadata(requires: CapabilityExpression) -> ModuleMetadata {
    fluid_module(
        MODULE_LIQUID_LOOK,
        "Liquid Look",
        "Draws the liquid's surface as water: tinted by what it absorbs, reflective at grazing \
         angles, with a highlight from one light.",
        requires,
    )
    .with_multiplicity(ModuleMultiplicity::Single)
    .with_inputs(vec![
        InputMetadata::new(
            "color",
            "Color",
            "The colour light takes on through the liquid.",
            Value::Vec3([0.18, 0.46, 0.72]),
            colour(),
        ),
        InputMetadata::new(
            "absorption",
            "Absorption",
            "How much light the liquid absorbs per unit of thickness: low is clear, high is opaque.",
            Value::Scalar(0.08),
            number(0.01, 0.0, None),
        ),
        InputMetadata::new(
            "surface_level",
            "Surface Level",
            "The liquid fraction the surface is drawn at: lower is fuller and blobbier, higher is \
             tighter and shows fewer stray drops.",
            Value::Scalar(0.5),
            number(0.05, 0.01, Some(4.0)),
        ),
        InputMetadata::new(
            "steps",
            "March Steps",
            "Samples along each view ray to find the surface: about one a cell.",
            Value::U32(96),
            number(1.0, 4.0, Some(MAX_VOLUME_STEPS as f32)),
        ),
        InputMetadata::new(
            "ambient",
            "Ambient",
            "Light reaching the liquid from every direction.",
            Value::Scalar(0.35),
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
            Value::Vec3([1.0, 0.97, 0.92]),
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
            "shininess",
            "Shininess",
            "How tight the highlight is.",
            Value::Scalar(96.0),
            number(1.0, 1.0, None),
        ),
    ])
    .with_cost(4)
}

/// Packs a Liquid Look's inputs into the constant words `liquid_look.wgsl` reads.
pub(super) fn pack_liquid_look(payload: &PropertyBag) -> Result<Vec<u32>, String> {
    let steps = count(payload, "steps")?;
    if !(1..=MAX_VOLUME_STEPS).contains(&steps) {
        return Err(format!(
            "march steps must be between 1 and {MAX_VOLUME_STEPS}, got {steps}"
        ));
    }
    let non_negative = |name: &str| {
        let value = scalar(payload, name)?;
        if value < 0.0 {
            return Err(format!("'{name}' must not be negative"));
        }
        Ok(value)
    };
    let level = scalar(payload, "surface_level")?;
    if level <= 0.0 {
        return Err("the surface level must be positive".into());
    }
    let direction = vec3(payload, "light_direction")?;
    let length = direction.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
    if length <= 1e-6 {
        return Err("the light direction must not be zero".into());
    }
    let mut words = vec![0u32; 16];
    words[0] = steps;
    words[1] = level.to_bits();
    words[2] = non_negative("absorption")?.to_bits();
    words[3] = non_negative("ambient")?.to_bits();
    for (axis, value) in vec3(payload, "color")?.into_iter().enumerate() {
        words[4 + axis] = value.to_bits();
    }
    words[7] = non_negative("light_intensity")?.to_bits();
    for axis in 0..3 {
        words[8 + axis] = (direction[axis] / length).to_bits();
    }
    words[11] = scalar(payload, "shininess")?.max(1.0).to_bits();
    for (axis, value) in vec3(payload, "light_color")?.into_iter().enumerate() {
        words[12 + axis] = value.to_bits();
    }
    Ok(words)
}

/// Validates a liquid module's resolved inputs.
pub(super) fn validate_liquid_module(
    module_type: &str,
    payload: &PropertyBag,
) -> Result<(), String> {
    match module_type {
        MODULE_LIQUID_GRID => {
            let resolution = count(payload, "resolution")?;
            if !(MIN_RESOLUTION..=MAX_RESOLUTION).contains(&resolution)
                || resolution % WORKGROUP != 0
            {
                return Err(format!(
                    "a liquid grid's resolution must be a multiple of {WORKGROUP} between \
                     {MIN_RESOLUTION} and {MAX_RESOLUTION}, got {resolution}"
                ));
            }
            if scalar(payload, "cell_size")? <= 0.0 {
                return Err("the liquid grid's cell size must be positive".into());
            }
            vec3(payload, "center")?;
            vec3(payload, "gravity")?;
            let budget = count(payload, "particle_budget")?;
            if !(1..=MAX_LIQUID_PARTICLES).contains(&budget) {
                return Err(format!(
                    "the particle budget must be between 1 and {MAX_LIQUID_PARTICLES}, got {budget}"
                ));
            }
            let substeps = count(payload, "substeps")?;
            if !(1..=MAX_LIQUID_SUBSTEPS).contains(&substeps) {
                return Err(format!(
                    "substeps must be between 1 and {MAX_LIQUID_SUBSTEPS}, got {substeps}"
                ));
            }
            let iterations = count(payload, "pressure_iterations")?;
            if !(1..=MAX_PRESSURE_ITERATIONS).contains(&iterations) {
                return Err(format!(
                    "pressure iterations must be between 1 and {MAX_PRESSURE_ITERATIONS}, \
                     got {iterations}"
                ));
            }
            if scalar(payload, "pressure_tolerance")? < 0.0 {
                return Err("the pressure tolerance must not be negative".into());
            }
        }
        MODULE_LIQUID_BLOCK => {
            vec3(payload, "center")?;
            vec3(payload, "velocity")?;
            if vec3(payload, "size")?.iter().any(|extent| *extent <= 0.0) {
                return Err("a liquid block's size must be positive on every axis".into());
            }
        }
        MODULE_LIQUID_SOURCE => {
            vec3(payload, "position")?;
            vec3(payload, "velocity")?;
            if scalar(payload, "radius")? <= 0.0 {
                return Err("a liquid source's radius must be positive".into());
            }
            if scalar(payload, "rate")? < 0.0 {
                return Err("a liquid source's rate must not be negative".into());
            }
        }
        MODULE_LIQUID_LOOK => {
            pack_liquid_look(payload)?;
        }
        other => return Err(format!("'{other}' is not a liquid module")),
    }
    Ok(())
}

/// A liquid stage's constants and what its lowering needs from them.
struct PackedLiquid {
    grid: Grid,
    cell_size: f32,
    origin: [f32; 3],
    constants: Vec<u32>,
    budget: u32,
    substeps: u32,
    iterations: u32,
    tolerance: f32,
    colliders: bool,
}

/// Packs the constants `liquid.wgsl` and the shared solver kernels read: the solver's header (a closed
/// box, no gas terms), the sources in the Density Source's record layout (the rate in its density
/// word), the colliders, then the liquid block — budget, substeps, gravity, the blocks — whose first
/// word header word 21 names.
fn pack_liquid(modules: &[ExtensionModulePlan]) -> Result<PackedLiquid, String> {
    let of = |type_id: &'static str| modules_of(modules, type_id);
    let mut grids = of(MODULE_LIQUID_GRID);
    let grid = grids
        .next()
        .ok_or("a Liquid Solver stage needs a Liquid Grid module")?;
    if grids.next().is_some() {
        return Err("a Liquid Solver stage takes one Liquid Grid module".into());
    }
    let sources: Vec<_> = of(MODULE_LIQUID_SOURCE).collect();
    if sources.len() > MAX_SOURCES {
        return Err(format!(
            "a Liquid Solver stage takes at most {MAX_SOURCES} liquid sources, got {}",
            sources.len()
        ));
    }
    let blocks: Vec<_> = of(MODULE_LIQUID_BLOCK).collect();
    if blocks.len() > MAX_LIQUID_BLOCKS {
        return Err(format!(
            "a Liquid Solver stage takes at most {MAX_LIQUID_BLOCKS} liquid blocks, got {}",
            blocks.len()
        ));
    }
    let parameters = &grid.parameters;
    let resolution = count(parameters, "resolution")?;
    let cell_size = scalar(parameters, "cell_size")?;
    let center = vec3(parameters, "center")?;
    let half_extent = resolution as f32 * cell_size * 0.5;
    let origin = center.map(|axis| axis - half_extent);
    let mut words = vec![0u32; SOURCE_BASE + sources.len() * SOURCE_WORDS];
    words[0] = resolution;
    words[1] = cell_size.to_bits();
    for axis in 0..3 {
        words[2 + axis] = origin[axis].to_bits();
    }
    words[9] = sources.len() as u32;
    for (index, source) in sources.iter().enumerate() {
        let base = SOURCE_BASE + index * SOURCE_WORDS;
        let position = vec3(&source.parameters, "position")?;
        let velocity = vec3(&source.parameters, "velocity")?;
        for axis in 0..3 {
            words[base + axis] = position[axis].to_bits();
            words[base + 4 + axis] = velocity[axis].to_bits();
        }
        words[base + 3] = scalar(&source.parameters, "radius")?.to_bits();
        words[base + 7] = scalar(&source.parameters, "rate")?.to_bits();
        words[base + 8..base + 11].copy_from_slice(&host_ref(source.host_fields.get("position"))?);
        words[base + 11..base + 14].copy_from_slice(&host_ref(source.host_fields.get("velocity"))?);
    }
    let colliders: Vec<_> = modules
        .iter()
        .filter(|module| is_collider(&module.module_type.0))
        .collect();
    if colliders.len() > MAX_COLLIDERS {
        return Err(format!(
            "a Liquid Solver stage takes at most {MAX_COLLIDERS} colliders, got {}",
            colliders.len()
        ));
    }
    words[12] = colliders.len() as u32;
    words[13] = words.len() as u32;
    for collider in &colliders {
        words.extend(pack_collider(collider)?);
    }
    let budget = count(parameters, "particle_budget")?;
    let substeps = count(parameters, "substeps")?;
    // Spatiotemporal FLIP (fluid F9).
    words[22] = u32::from(parameters.get_bool("spatiotemporal").unwrap_or(false));
    words[21] = words.len() as u32;
    words.push(budget);
    words.push(substeps);
    words.extend(vec3(parameters, "gravity")?.map(f32::to_bits));
    words.push(blocks.len() as u32);
    for block in &blocks {
        let center = vec3(&block.parameters, "center")?;
        let size = vec3(&block.parameters, "size")?;
        let velocity = vec3(&block.parameters, "velocity")?;
        let dims = size.map(|extent| ((extent / cell_size).round() as u32).max(1));
        let mut record = [0u32; BLOCK_WORDS];
        for axis in 0..3 {
            record[axis] = (center[axis] - 0.5 * size[axis]).to_bits();
            record[3 + axis] = velocity[axis].to_bits();
            record[6 + axis] = dims[axis];
        }
        record[9] = dims
            .iter()
            .map(|&n| u64::from(n))
            .product::<u64>()
            .saturating_mul(8)
            .min(u64::from(MAX_LIQUID_PARTICLES)) as u32;
        words.extend(record);
    }
    Ok(PackedLiquid {
        grid: Grid {
            resolution,
            slots: None,
            liquid: true,
        },
        cell_size,
        origin,
        constants: words,
        budget,
        substeps,
        iterations: count(parameters, "pressure_iterations")?,
        tolerance: scalar(parameters, "pressure_tolerance")?,
        colliders: !colliders.is_empty(),
    })
}

/// Lowers a Liquid Solver stage (see the module docs).
pub(super) struct LiquidSolverLowerer;

impl StageLowerer for LiquidSolverLowerer {
    /// A Liquid Look draws the liquid fraction's surface as water.
    fn present(
        &self,
        input: &StageLoweringInput<'_>,
        _block: &ExecutionBlock,
    ) -> Result<Vec<StagePresentation>, String> {
        modules_of(input.modules, MODULE_LIQUID_LOOK)
            .map(|look| {
                Ok(StagePresentation::Volume(VolumePresentation {
                    program: ComputeProgramId::new(PROGRAM_LIQUID_LOOK),
                    entry_point: LIQUID_LOOK_ENTRY.into(),
                    fields: vec![ResourceTypeId::new(RESOURCE_DENSITY)],
                    constants: pack_liquid_look(&look.parameters)?,
                }))
            })
            .collect()
    }

    fn lower(&self, input: &StageLoweringInput<'_>) -> Result<ExecutionBlock, String> {
        use ResourceAccess as Access;
        let PackedLiquid {
            grid,
            cell_size,
            origin,
            constants,
            budget,
            substeps,
            iterations,
            tolerance,
            colliders,
        } = pack_liquid(input.modules)?;
        let constants_read = || Access::read(AESTRA_RESOURCE_STAGE_CONSTANTS);
        let frame_read = || Access::read(AESTRA_RESOURCE_FRAME);
        let host_read = || Access::read(AESTRA_RESOURCE_HOST_BINDINGS);
        // A pass over the particles, sized on the device from the plan (word 0: this tick's new
        // particles; 4: all of them), bounded by the budget.
        let particles = |entry: &str, accesses: Vec<ResourceAccess>, word: u32| {
            grid.op(
                entry,
                accesses,
                (
                    StagedDispatch {
                        x: budget.div_ceil(64),
                        y: 1,
                        z: 1,
                    },
                    Some(IndirectDispatch {
                        resource: ResourceTypeId::new(RESOURCE_LIQUID_DISPATCH),
                        word,
                    }),
                ),
            )
        };

        let mut steps = Vec::new();
        if colliders {
            steps.push(grid.pass(
                "mark_solids",
                vec![
                    Access::write(RESOURCE_SOLID),
                    Access::read_write(RESOURCE_DENSITY),
                    constants_read(),
                    frame_read(),
                    host_read(),
                ],
            ));
        }
        steps.push(grid.single(
            "liquid_plan",
            vec![
                Access::read_write(RESOURCE_LIQUID_HEADER),
                Access::write(RESOURCE_LIQUID_DISPATCH),
                constants_read(),
                frame_read(),
            ],
        ));
        steps.push(particles(
            "liquid_emit",
            vec![
                Access::write(RESOURCE_LIQUID_PARTICLES),
                Access::read(RESOURCE_LIQUID_HEADER),
                constants_read(),
                frame_read(),
                host_read(),
            ],
            0,
        ));
        for _ in 0..substeps {
            steps.push(grid.pass(
                "liquid_clear",
                vec![Access::write(RESOURCE_LIQUID_TRANSFER), constants_read()],
            ));
            steps.push(particles(
                "liquid_p2g",
                vec![
                    Access::read(RESOURCE_LIQUID_PARTICLES),
                    Access::read_write(RESOURCE_LIQUID_TRANSFER),
                    constants_read(),
                    frame_read(),
                ],
                4,
            ));
            steps.push(grid.pass(
                "liquid_faces",
                vec![
                    Access::read(RESOURCE_LIQUID_TRANSFER),
                    Access::write(RESOURCE_VELOCITY),
                    constants_read(),
                    frame_read(),
                ],
            ));
            steps.push(grid.pass(
                "liquid_mark",
                vec![
                    Access::read(RESOURCE_LIQUID_TRANSFER),
                    Access::write(RESOURCE_DENSITY),
                    Access::write(RESOURCE_MG_FLAGS),
                    Access::write(RESOURCE_PRESSURE),
                    Access::read(RESOURCE_SOLID),
                    // The spatiotemporal pressure coefficients (fluid F9).
                    Access::write(RESOURCE_VELOCITY_HAT),
                    constants_read(),
                ],
            ));
            steps.push(grid.pass(
                "compute_divergence",
                vec![
                    Access::read(RESOURCE_VELOCITY),
                    Access::write(RESOURCE_DIVERGENCE),
                    Access::read(RESOURCE_SOLID),
                    constants_read(),
                ],
            ));
            steps.extend(multigrid_pressure(
                grid, iterations, tolerance, colliders, true,
            ));
            steps.push(grid.pass(
                "project",
                vec![
                    Access::read_write(RESOURCE_VELOCITY),
                    Access::read(RESOURCE_PRESSURE),
                    Access::read(RESOURCE_SOLID),
                    Access::read(RESOURCE_VELOCITY_HAT),
                    constants_read(),
                ],
            ));
            steps.push(particles(
                "liquid_g2p",
                vec![
                    Access::read_write(RESOURCE_LIQUID_PARTICLES),
                    Access::read(RESOURCE_VELOCITY),
                    constants_read(),
                    frame_read(),
                    host_read(),
                ],
                4,
            ));
        }

        let cells = grid.cells();
        let mut resources = super::resources(grid, constants.len(), false, true, 0);
        // The fire grids' bindings, unused: the liquid's follow them.
        for id in [
            RESOURCE_TEMPERATURE,
            RESOURCE_TEMPERATURE_NEXT,
            RESOURCE_FUEL,
            RESOURCE_FUEL_NEXT,
        ] {
            resources.push(ResourceDescriptor {
                id: ResourceTypeId::new(id),
                bytes: 16,
                lifetime: ResourceLifetime::Transient,
            });
        }
        let resource = |id: &str, bytes: u64, lifetime| ResourceDescriptor {
            id: ResourceTypeId::new(id),
            bytes,
            lifetime,
        };
        resources.extend([
            resource(
                RESOURCE_LIQUID_PARTICLES,
                u64::from(budget) * PARTICLE_BYTES,
                ResourceLifetime::Persistent,
            ),
            resource(RESOURCE_LIQUID_HEADER, 16, ResourceLifetime::Persistent),
            resource(RESOURCE_LIQUID_DISPATCH, 32, ResourceLifetime::Transient),
            resource(
                RESOURCE_LIQUID_TRANSFER,
                cells * 32,
                ResourceLifetime::Transient,
            ),
        ]);
        let field = |id: &str, components, staggered| FieldLayout {
            resource: ResourceTypeId::new(id),
            dims: [grid.resolution; 3],
            components,
            origin,
            cell_size,
            staggered,
            bricks: None,
        };
        Ok(ExecutionBlock {
            resources,
            ops: with_barriers(steps),
            constants,
            // The grid velocity, and the liquid fraction (1 where the liquid is full).
            fields: vec![
                field(RESOURCE_VELOCITY, 4, true),
                field(RESOURCE_DENSITY, 1, false),
            ],
        })
    }
}

/// An effect whose own *Liquid Solver* domain holds a Liquid Grid and a Liquid Block — every input at
/// its schema default: a dam break. `registry` must have the extension installed.
pub fn liquid_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = EffectAsset::new("Liquid", 6.0);
    let mut domain =
        EffectSimulationStage::new(LIQUID_STAGE, StageTypeId::new(STAGE_LIQUID_SOLVER));
    for type_id in [MODULE_LIQUID_GRID, MODULE_LIQUID_BLOCK] {
        let mut module = registry
            .modules
            .instantiate(&ModuleTypeId::new(type_id))
            .expect("the fluid extension is installed in the registry");
        module.stage = StageKind::Simulation(LIQUID_STAGE.into());
        domain.modules.push(module);
    }
    effect.simulation_stages.push(domain);
    effect
}

/// The simulation-stage name [`liquid_effect`] authors.
pub const LIQUID_STAGE: &str = "Liquid";
