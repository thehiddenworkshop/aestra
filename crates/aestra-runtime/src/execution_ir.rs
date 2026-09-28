//! Portable Execution IR and resource model (extensible-stages redesign M6).
//!
//! A compiled stage ([`crate::CompiledStage`]) lowers to an [`ExecutionBlock`]: an ordered list of
//! [`ExecutionOp`]s — compute passes, barriers, copies, and repeat loops — over declared
//! [`ResourceDescriptor`]s. This lets one stage lower into **more than one** runtime/GPU pass (a solver
//! iteration, a ping-pong, a multi-stage fluid), while still allowing the common case to stay **fused**
//! (a whole particle stage is one compute op, not one dispatch per module — see [`lower_stage_fused`]).
//! Nothing here depends on a GPU API; a reference backend ([`execute_reference`]) runs a block into a
//! deterministic ordered trace so the IR's ordering and repeat/barrier semantics can be validated
//! without hardware. The native GPU backend consumes the same IR in M7.
//!
//! ## Binding convention
//!
//! A compute op's program sees the block's resources as storage buffers at `@group(0)`, each at
//! `@binding(i)` where `i` is the resource's index in [`ExecutionBlock::resources`]. An op's
//! [`ComputeOp::accesses`] must name exactly the resources its entry point statically uses: the backend
//! binds those and nothing else. Two host-written built-ins complete the inputs a program can read:
//! [`AESTRA_RESOURCE_STAGE_CONSTANTS`] (the block's [`ExecutionBlock::constants`], fixed at compile
//! time) and [`AESTRA_RESOURCE_FRAME`] (a [`FrameConstants`] written every tick).

use crate::{CompiledStage, StagedDispatch};
use aestra_core::{ComputeProgramId, ResourceTypeId};

/// The built-in resource id for an emitter's particle buffer — what the standard fused particle stages
/// read and write.
pub const AESTRA_RESOURCE_PARTICLES: &str = "aestra.resource.particles";

/// The built-in resource holding an effect instance's host binding snapshots, packed per the
/// `aestra_gpu` host-binding ABI (host bindings HB6). Host-written once per tick, read-only to stages:
/// a plugin stage that reads bound objects declares it and a `Read` access on its compute ops.
pub const AESTRA_RESOURCE_HOST_BINDINGS: &str = "aestra.resource.host_bindings";

/// The built-in resource holding the host's world SDF, packed per the `aestra_gpu` world-SDF ABI
/// (fluid F11, host bindings HB10): the scene geometry a simulation collides with, as a
/// [`crate::SdfVolume`]. Host-written when the world changes, sized by the host, read-only to stages;
/// never part of a stage's checkpoints. A stage that collides with the world declares it (bytes 0)
/// and a `Read` access on its compute ops; without a world it reads as absent.
pub const AESTRA_RESOURCE_WORLD_SDF: &str = "aestra.resource.world_sdf";

/// The domain of host-supplied inputs (host bindings HB6).
pub const AESTRA_DOMAIN_HOST_INPUT: &str = "aestra.domain.host_input";

/// The built-in resource holding a block's [`ExecutionBlock::constants`] — the words a stage lowerer
/// packed from its modules' resolved parameters (extensible-stages M13). Uploaded once, read-only.
pub const AESTRA_RESOURCE_STAGE_CONSTANTS: &str = "aestra.resource.stage_constants";

/// The built-in resource holding the current tick's [`FrameConstants`] (extensible-stages M13).
/// Host-written before every tick, read-only to stages.
pub const AESTRA_RESOURCE_FRAME: &str = "aestra.resource.frame";

/// Per-tick values the host writes to [`AESTRA_RESOURCE_FRAME`] (extensible-stages M13, fluid F2), as
/// sixteen 32-bit words: `tick: u32`, `dt: f32`, `time: f32`, `seed: u32`, then `world_to_effect` — the
/// 3×4 affine (rows, `f32`) taking world space into the effect's space. A stage's domain lives in its
/// effect's space and so moves rigidly with it; world-space host inputs (bound positions and
/// velocities) are converted with this. A staged simulation is a pure function of its asset, its seed
/// and this sequence — which is what makes GPU-vs-GPU reruns reproducible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameConstants {
    pub tick: u32,
    pub dt: f32,
    pub time: f32,
    pub seed: u32,
    pub world_to_effect: [[f32; 4]; 3],
}

/// The identity 3×4 affine.
pub const IDENTITY_AFFINE: [[f32; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

impl FrameConstants {
    /// Size of the frame resource in bytes.
    pub const BYTES: u64 = 64;

    /// The fixed-step frame for `tick`: `time = tick * dt`, the effect placed at the world origin.
    pub fn fixed_step(tick: u32, dt: f32, seed: u32) -> Self {
        Self {
            tick,
            dt,
            time: tick as f32 * dt,
            seed,
            world_to_effect: IDENTITY_AFFINE,
        }
    }

    /// The same frame with the effect placed by `world_to_effect`.
    pub fn with_world_to_effect(mut self, world_to_effect: [[f32; 4]; 3]) -> Self {
        self.world_to_effect = world_to_effect;
        self
    }

    /// The words uploaded to [`AESTRA_RESOURCE_FRAME`].
    pub fn to_words(self) -> [u32; 16] {
        let mut words = [0u32; 16];
        words[..4].copy_from_slice(&[self.tick, self.dt.to_bits(), self.time.to_bits(), self.seed]);
        for (row, values) in self.world_to_effect.iter().enumerate() {
            for (column, value) in values.iter().enumerate() {
                words[4 + row * 4 + column] = value.to_bits();
            }
        }
        words
    }
}
/// How an [`ExecutionOp`] accesses a resource (extensible-stages M6). Drives barrier/hazard reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceAccessMode {
    Read,
    Write,
    ReadWrite,
}

/// One resource an op reads and/or writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceAccess {
    pub resource: ResourceTypeId,
    pub mode: ResourceAccessMode,
}

impl ResourceAccess {
    pub fn read(resource: impl Into<String>) -> Self {
        Self {
            resource: ResourceTypeId::new(resource),
            mode: ResourceAccessMode::Read,
        }
    }

    pub fn write(resource: impl Into<String>) -> Self {
        Self {
            resource: ResourceTypeId::new(resource),
            mode: ResourceAccessMode::Write,
        }
    }

    pub fn read_write(resource: impl Into<String>) -> Self {
        Self {
            resource: ResourceTypeId::new(resource),
            mode: ResourceAccessMode::ReadWrite,
        }
    }
}

/// Whether a declared resource persists across ticks (checkpointed) or is transient scratch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceLifetime {
    Persistent,
    Transient,
}

/// A resource an execution block operates on (extensible-stages M6): a typed buffer of `bytes` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDescriptor {
    pub id: ResourceTypeId,
    pub bytes: u64,
    pub lifetime: ResourceLifetime,
}

/// A compute pass: a named kernel entry, the resources it accesses, and its dispatch shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputeOp {
    pub name: String,
    /// The registered program holding `entry_point` (extensible plan §13.2), or `None` for a
    /// backend-provided kernel (the built-in fused particle stages).
    pub program: Option<ComputeProgramId>,
    pub entry_point: String,
    pub accesses: Vec<ResourceAccess>,
    /// The workgroup counts; with [`Self::indirect`], the most the device-written counts may be.
    pub dispatch: StagedDispatch,
    /// Workgroup counts the device decides (fluid F7, G7): read when the op runs, so a pass can cover
    /// only what earlier ops found to be live (a sparse grid's active bricks) without a readback.
    pub indirect: Option<IndirectDispatch>,
}

/// Where an indirect [`ComputeOp`] reads its workgroup counts: the three `u32` words x, y, z starting
/// at word `word` of `resource`, written by earlier ops. They must not exceed the op's `dispatch` —
/// the bound budgets and pass counts are planned with — and a count of zero dispatches nothing. The
/// resource needs no access of its own on the op: the dispatch reads it, not the program. When the op
/// does bind it, it may only read it (a dispatch cannot read its counts from a buffer it writes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndirectDispatch {
    pub resource: ResourceTypeId,
    pub word: u32,
}

/// A copy between two resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyOp {
    pub from: ResourceTypeId,
    pub to: ResourceTypeId,
}

/// How many times a [`ExecutionOp::Repeat`] body runs (extensible-stages M6, fluid F5).
#[derive(Debug, Clone, PartialEq)]
pub enum RepeatPolicy {
    /// Run the body exactly `n` times (a fixed solver-iteration count).
    FixedCount(u32),
    /// Run the body until the `f32` in word 0 of `residual` is at or below `tolerance` after an
    /// iteration, at most `max` times (fluid F5: an iterative solve that stops once converged).
    ///
    /// The test runs **on the device, with no readback**: after each iteration the backend compares
    /// the residual and, once it has converged, turns the remaining iterations' dispatches into empty
    /// ones. So the body writes `residual` every iteration and holds only compute ops and barriers (a
    /// copy cannot be skipped on the device). A residual that is not a number never converges: the
    /// body then runs `max` times. The iterations that ran depend only on the stage's values, so a
    /// rerun repeats them exactly.
    ///
    /// With `test_first`, the residual is also tested before the first iteration (fluid F6), so the
    /// ops before the repeat must have written it: a solve that starts converged — nothing to solve,
    /// or a warm start already good enough — runs no iteration at all.
    UntilConverged {
        residual: ResourceTypeId,
        tolerance: f32,
        max: u32,
        test_first: bool,
    },
}

impl RepeatPolicy {
    /// The most times the body runs: exactly for a fixed count, the cap for a convergent repeat.
    pub fn count(&self) -> u32 {
        match self {
            Self::FixedCount(n) => *n,
            Self::UntilConverged { max, .. } => *max,
        }
    }
}

/// One operation in an [`ExecutionBlock`].
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionOp {
    /// A compute pass.
    Compute(ComputeOp),
    /// An execution barrier — the ops before it complete before the ops after it begin.
    Barrier,
    /// A resource-to-resource copy.
    Copy(CopyOp),
    /// Repeat a sub-sequence of ops (e.g. a pressure-solver iteration loop).
    Repeat {
        policy: RepeatPolicy,
        body: Vec<ExecutionOp>,
    },
}

/// Declares that a resource holds a regular 3-D grid field (fluid F1): `dims` cells, x fastest, each
/// `components` consecutive `f32`s (a 3-component field is padded to 4 — GPU `vec4` alignment), placed
/// in space at `origin` (the grid's minimum corner) with cubic cells of `cell_size`. Generic metadata:
/// tools and renderers use it to interpret the bytes (debug slices now; field sampling and volume
/// rendering later) without knowing what the field means.
///
/// A **staggered** vector field (fluid F4, a MAC grid) stores component `c` of cell `i` on the cell's
/// minimum face along axis `c` — at `origin + (i + 0.5 - 0.5·e_c)·cell_size` — rather than at its
/// centre; samplers offset each component accordingly.
///
/// A **bricked** field (fluid F7) stores only some of its bricks: see [`BrickLayout`].
#[derive(Debug, Clone, PartialEq)]
pub struct FieldLayout {
    pub resource: ResourceTypeId,
    pub dims: [u32; 3],
    pub components: u32,
    pub origin: [f32; 3],
    pub cell_size: f32,
    pub staggered: bool,
    pub bricks: Option<BrickLayout>,
}

/// How a bricked [`FieldLayout`] is stored (fluid F7): the grid is cut into `edge`³-cell bricks, and
/// only some are stored, each in one of `slots` slots of `edge`³ cells (x fastest within the brick),
/// so cell `c` of the brick in slot `s` is element `s·edge³ + c`. Two `u32` arrays of the resource
/// `table` say which: at word `table_word`, one entry per brick (x fastest over the dims / edge
/// bricks) holding its slot, 0 when it is not stored — its cells then read as zero; at word
/// `slot_bricks_word`, one entry per slot holding its brick's coordinates packed as
/// `x | y << 10 | z << 20`, plus one, 0 for an unused slot. Slot 0 is never a brick's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrickLayout {
    pub edge: u32,
    pub slots: u32,
    pub table: ResourceTypeId,
    pub table_word: u32,
    pub slot_bricks_word: u32,
}

impl BrickLayout {
    /// Bricks along each axis of a grid of `dims`.
    pub fn grid(&self, dims: [u32; 3]) -> [u32; 3] {
        dims.map(|dim| dim / self.edge)
    }
}

impl FieldLayout {
    /// Cells in the grid.
    pub fn cells(&self) -> u64 {
        self.dims.iter().map(|&dim| u64::from(dim)).product()
    }

    /// Cells the resource stores: every cell, or every slot's brick.
    pub fn stored_cells(&self) -> u64 {
        match &self.bricks {
            Some(bricks) => u64::from(bricks.slots) * u64::from(bricks.edge).pow(3),
            None => self.cells(),
        }
    }

    /// Bytes one cell occupies (`vec4` alignment for 3 components).
    pub fn cell_bytes(&self) -> u64 {
        u64::from(if self.components == 3 {
            4
        } else {
            self.components
        }) * 4
    }
}

/// Declares that a resource is an emission list (fluid F10, G8): the points a stage asks particles to
/// be born at this tick, which an emitter spawning from the stage turns into particles on the device,
/// with no readback. Word 0 holds how many records the stage emitted (a spawner reads at most
/// `capacity`); words 1..4 are reserved; record `i` is the [`Self::RECORD_WORDS`] `f32`s from word
/// `4 + 8i`: a position (`xyz`, `w` reserved) then a velocity (`xyz`, `w` reserved), in the stage's
/// space. The stage rewrites it every tick in an order that depends only on its state — a scan, never
/// an atomic append — so the particles spawned from it reproduce on a rerun.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmissionLayout {
    pub resource: ResourceTypeId,
    pub capacity: u32,
}

impl EmissionLayout {
    /// Words before the first record.
    pub const HEADER_WORDS: u32 = 4;
    /// Words per record.
    pub const RECORD_WORDS: u32 = 8;

    /// Bytes the list occupies at its capacity.
    pub fn bytes(&self) -> u64 {
        (u64::from(Self::HEADER_WORDS) + u64::from(self.capacity) * u64::from(Self::RECORD_WORDS))
            * 4
    }
}

/// A value a stage reports to the host (fluid F11, host bindings HB9) — a reduced output such as the
/// net force on a collider: `components` `f32` words from word `word` of `resource`. The host reads
/// the resource back after each frame's ticks and then zeroes it, so what a stage keeps there is its
/// own business per frame (the fluid keeps the frame's strongest tick); such a resource is the host's,
/// never part of the stage's checkpoints. With `event`, the host also raises a runtime event when the
/// value's magnitude rises past a threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct StageOutput {
    /// What gameplay knows the value by, e.g. `force`.
    pub name: String,
    /// The authored module the value belongs to (a collider), when it belongs to one.
    pub source: Option<aestra_core::ModuleId>,
    pub resource: ResourceTypeId,
    pub word: u32,
    pub components: u32,
    pub event: Option<OutputEvent>,
}

/// The runtime event an output raises when its magnitude rises past `threshold`: its value was at or
/// below it at the previous read (or there was none) and is above it now.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputEvent {
    /// What gameplay knows the event by, e.g. `impact`.
    pub kind: String,
    pub threshold: f32,
}

/// The ordered execution plan of one stage (extensible-stages M6): declared resources plus the ops
/// that run over them, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecutionBlock {
    pub resources: Vec<ResourceDescriptor>,
    pub ops: Vec<ExecutionOp>,
    /// The contents of [`AESTRA_RESOURCE_STAGE_CONSTANTS`] when the block declares it: parameters
    /// the stage lowerer packed at compile time, in a layout private to the stage's programs.
    pub constants: Vec<u32>,
    /// Grid-field layouts of some of the block's resources (fluid F1).
    pub fields: Vec<FieldLayout>,
    /// Emission lists among the block's resources (fluid F10).
    pub emissions: Vec<EmissionLayout>,
    /// Values the stage reports to the host (fluid F11).
    pub outputs: Vec<StageOutput>,
}

/// Why an [`ExecutionBlock`] is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    /// An op references a resource the block does not declare.
    UnknownResource(ResourceTypeId),
    /// A resource id is declared twice.
    DuplicateResource(ResourceTypeId),
    /// A compute op has a zero dispatch shape.
    EmptyDispatch(String),
    /// A repeat policy would run its body zero times.
    ZeroRepeat,
    /// A built-in host-written resource is declared with the wrong size or lifetime.
    InvalidBuiltinResource(ResourceTypeId),
    /// A field layout names an undeclared resource, is empty, or does not fit its resource.
    InvalidField(ResourceTypeId),
    /// An emission list names an undeclared resource, has no capacity, or does not fit its resource.
    InvalidEmission(ResourceTypeId),
    /// An output names an undeclared or non-persistent resource, has no components, does not fit its
    /// resource, or has a threshold that is not a finite, non-negative number.
    InvalidOutput(String),
    /// A convergent repeat's body holds a copy or a nested repeat, or its tolerance is not a finite,
    /// non-negative number.
    InvalidConvergentRepeat,
    /// An indirect compute op's counts do not fit in their resource, or the op writes that resource.
    InvalidIndirect(String),
}

impl core::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownResource(id) => {
                write!(f, "op references undeclared resource '{}'", id.as_str())
            }
            Self::DuplicateResource(id) => {
                write!(f, "resource '{}' is declared twice", id.as_str())
            }
            Self::EmptyDispatch(name) => write!(f, "compute op '{name}' has a zero dispatch shape"),
            Self::ZeroRepeat => write!(f, "a repeat policy would run its body zero times"),
            Self::InvalidConvergentRepeat => write!(
                f,
                "a convergent repeat holds only compute ops and barriers, with a finite, \
                 non-negative tolerance"
            ),
            Self::InvalidIndirect(name) => write!(
                f,
                "compute op '{name}' reads its workgroup counts past the end of their resource, or \
                 from a resource it writes"
            ),
            Self::InvalidField(id) => write!(
                f,
                "field layout of '{}' is empty or does not fit the declared resource",
                id.as_str()
            ),
            Self::InvalidEmission(id) => write!(
                f,
                "emission list '{}' has no capacity or does not fit the declared resource",
                id.as_str()
            ),
            Self::InvalidOutput(name) => write!(
                f,
                "output '{name}' does not fit a persistent resource of the block, or its event's \
                 threshold is not a finite, non-negative number"
            ),
            Self::InvalidBuiltinResource(id) => write!(
                f,
                "built-in resource '{}' must be declared persistent with its fixed size",
                id.as_str()
            ),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl ExecutionBlock {
    /// Validates the block: resources are declared once, every op's resource accesses (and copy
    /// endpoints) resolve to a declared resource, compute dispatches are non-zero, and repeat policies
    /// run at least once. Recurses into repeat bodies.
    pub fn validate(&self) -> Result<(), ExecutionError> {
        let mut declared = std::collections::BTreeSet::new();
        for resource in &self.resources {
            if !declared.insert(resource.id.clone()) {
                return Err(ExecutionError::DuplicateResource(resource.id.clone()));
            }
            let fixed_bytes = match resource.id.as_str() {
                AESTRA_RESOURCE_FRAME => Some(FrameConstants::BYTES),
                AESTRA_RESOURCE_STAGE_CONSTANTS => Some(self.constants.len() as u64 * 4),
                _ => None,
            };
            if let Some(bytes) = fixed_bytes
                && (resource.bytes != bytes || resource.lifetime != ResourceLifetime::Persistent)
            {
                return Err(ExecutionError::InvalidBuiltinResource(resource.id.clone()));
            }
        }
        let bytes_of = |id: &ResourceTypeId| {
            self.resources
                .iter()
                .find(|resource| &resource.id == id)
                .map(|resource| resource.bytes)
        };
        for field in &self.fields {
            let bricks_fit = field.bricks.as_ref().is_none_or(|bricks| {
                let grid = bricks.grid(field.dims);
                let entries = grid.iter().map(|&n| u64::from(n)).product::<u64>();
                bricks.edge > 0
                    && bricks.slots > 0
                    && field.dims.iter().all(|dim| dim % bricks.edge == 0)
                    && bytes_of(&bricks.table).is_some_and(|bytes| {
                        bytes >= (u64::from(bricks.table_word) + entries) * 4
                            && bytes
                                >= (u64::from(bricks.slot_bricks_word) + u64::from(bricks.slots))
                                    * 4
                    })
            });
            let fits = bricks_fit
                && bytes_of(&field.resource).is_some_and(|bytes| {
                    (1..=4).contains(&field.components)
                        && field.cells() > 0
                        && field.cell_size > 0.0
                        && bytes >= field.stored_cells() * field.cell_bytes()
                });
            if !fits {
                return Err(ExecutionError::InvalidField(field.resource.clone()));
            }
        }
        for emission in &self.emissions {
            if emission.capacity == 0
                || !bytes_of(&emission.resource).is_some_and(|bytes| bytes >= emission.bytes())
            {
                return Err(ExecutionError::InvalidEmission(emission.resource.clone()));
            }
        }
        for output in &self.outputs {
            let persistent = self.resources.iter().any(|resource| {
                resource.id == output.resource && resource.lifetime == ResourceLifetime::Persistent
            });
            let fits = bytes_of(&output.resource).is_some_and(|bytes| {
                bytes >= (u64::from(output.word) + u64::from(output.components)) * 4
            });
            let threshold_ok = output
                .event
                .as_ref()
                .is_none_or(|event| event.threshold.is_finite() && event.threshold >= 0.0);
            if !persistent || output.components == 0 || !fits || !threshold_ok {
                return Err(ExecutionError::InvalidOutput(output.name.clone()));
            }
        }
        let sizes = self
            .resources
            .iter()
            .map(|resource| (resource.id.clone(), resource.bytes))
            .collect();
        validate_ops(&self.ops, &sizes)
    }

    /// The field layout of a resource, if the block declares one.
    pub fn field(&self, id: &ResourceTypeId) -> Option<&FieldLayout> {
        self.fields.iter().find(|field| &field.resource == id)
    }

    /// Whether `id` holds outputs (fluid F11): the host's, never checkpointed.
    pub fn is_output(&self, id: &ResourceTypeId) -> bool {
        self.outputs.iter().any(|output| &output.resource == id)
    }

    /// The emission list of a resource, if the block declares one.
    pub fn emission(&self, id: &ResourceTypeId) -> Option<&EmissionLayout> {
        self.emissions
            .iter()
            .find(|emission| &emission.resource == id)
    }

    /// The binding index of a declared resource — its position in [`Self::resources`].
    pub fn binding_of(&self, id: &ResourceTypeId) -> Option<u32> {
        self.resources
            .iter()
            .position(|resource| &resource.id == id)
            .map(|index| index as u32)
    }

    /// The descriptor for the stage-constants resource holding [`Self::constants`].
    pub fn constants_resource(&self) -> ResourceDescriptor {
        ResourceDescriptor {
            id: ResourceTypeId::new(AESTRA_RESOURCE_STAGE_CONSTANTS),
            bytes: self.constants.len() as u64 * 4,
            lifetime: ResourceLifetime::Persistent,
        }
    }

    /// The descriptor for the per-tick frame resource.
    pub fn frame_resource() -> ResourceDescriptor {
        ResourceDescriptor {
            id: ResourceTypeId::new(AESTRA_RESOURCE_FRAME),
            bytes: FrameConstants::BYTES,
            lifetime: ResourceLifetime::Persistent,
        }
    }

    /// The number of compute passes a run of this block performs, with repeats expanded — the count of
    /// GPU dispatches (and of pass-level timestamp intervals) the native backend will issue. A
    /// convergent repeat counts at its cap (the dispatches are issued; past convergence they are
    /// empty).
    pub fn compute_pass_count(&self) -> u32 {
        count_compute(&self.ops)
    }
}

/// Declared resources and their sizes in bytes (0: sized by the backend).
type DeclaredResources = std::collections::BTreeMap<ResourceTypeId, u64>;

fn validate_ops(ops: &[ExecutionOp], declared: &DeclaredResources) -> Result<(), ExecutionError> {
    for op in ops {
        match op {
            ExecutionOp::Compute(compute) => {
                if !compute.dispatch.is_nonzero() {
                    return Err(ExecutionError::EmptyDispatch(compute.name.clone()));
                }
                for access in &compute.accesses {
                    if !declared.contains_key(&access.resource) {
                        return Err(ExecutionError::UnknownResource(access.resource.clone()));
                    }
                }
                if let Some(indirect) = &compute.indirect {
                    let bytes = declared.get(&indirect.resource).ok_or_else(|| {
                        ExecutionError::UnknownResource(indirect.resource.clone())
                    })?;
                    let written = compute.accesses.iter().any(|access| {
                        access.resource == indirect.resource
                            && access.mode != ResourceAccessMode::Read
                    });
                    if u64::from(indirect.word) * 4 + 12 > *bytes || written {
                        return Err(ExecutionError::InvalidIndirect(compute.name.clone()));
                    }
                }
            }
            ExecutionOp::Copy(copy) => {
                for id in [&copy.from, &copy.to] {
                    if !declared.contains_key(id) {
                        return Err(ExecutionError::UnknownResource(id.clone()));
                    }
                }
            }
            ExecutionOp::Barrier => {}
            ExecutionOp::Repeat { policy, body } => {
                if policy.count() == 0 {
                    return Err(ExecutionError::ZeroRepeat);
                }
                if let RepeatPolicy::UntilConverged {
                    residual,
                    tolerance,
                    ..
                } = policy
                {
                    if !declared.contains_key(residual) {
                        return Err(ExecutionError::UnknownResource(residual.clone()));
                    }
                    let skippable = body
                        .iter()
                        .all(|op| matches!(op, ExecutionOp::Compute(_) | ExecutionOp::Barrier));
                    if !skippable || !tolerance.is_finite() || *tolerance < 0.0 {
                        return Err(ExecutionError::InvalidConvergentRepeat);
                    }
                }
                validate_ops(body, declared)?;
            }
        }
    }
    Ok(())
}

fn count_compute(ops: &[ExecutionOp]) -> u32 {
    ops.iter()
        .map(|op| match op {
            ExecutionOp::Compute(_) => 1,
            ExecutionOp::Repeat { policy, body } => policy.count() * count_compute(body),
            _ => 0,
        })
        .sum()
}

/// The ordered trace a reference run of an [`ExecutionBlock`] produces (extensible-stages M6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferenceExecutionTrace {
    /// One entry per executed step, in order: `compute:<name>`, `barrier`, or `copy:<from>-><to>`.
    /// Repeat loops are expanded, so a body run four times appears four times.
    pub steps: Vec<String>,
}

/// Runs a block through a reference/mock backend into a deterministic ordered trace — no GPU. Repeats
/// expand and barriers appear in place, so the trace is exactly the order the native backend must
/// issue its passes in (extensible-stages M6).
pub fn execute_reference(block: &ExecutionBlock) -> ReferenceExecutionTrace {
    let mut steps = Vec::new();
    trace_ops(&block.ops, &mut steps);
    ReferenceExecutionTrace { steps }
}

fn trace_ops(ops: &[ExecutionOp], steps: &mut Vec<String>) {
    for op in ops {
        match op {
            ExecutionOp::Compute(compute) => steps.push(format!("compute:{}", compute.name)),
            ExecutionOp::Barrier => steps.push("barrier".to_string()),
            ExecutionOp::Copy(copy) => {
                steps.push(format!("copy:{}->{}", copy.from.as_str(), copy.to.as_str()))
            }
            ExecutionOp::Repeat { policy, body } => {
                if let RepeatPolicy::UntilConverged {
                    residual,
                    test_first: true,
                    ..
                } = policy
                {
                    steps.push(format!("converged?:{}", residual.as_str()));
                }
                for _ in 0..policy.count() {
                    trace_ops(body, steps);
                    // The reference backend holds no values: it traces every iteration up to the
                    // cap, each followed by the device-side test that may empty the rest.
                    if let RepeatPolicy::UntilConverged { residual, .. } = policy {
                        steps.push(format!("converged?:{}", residual.as_str()));
                    }
                }
            }
        }
    }
}

/// Lowers a compiled stage to a **fused** execution block (extensible-stages M6): the whole stage is a
/// single compute pass reading and writing the particle buffer — its modules are *not* forced into
/// separate dispatches, preserving the current fused execution. Stages that genuinely need multiple
/// passes (solvers, grids) build their own richer [`ExecutionBlock`] instead.
pub fn lower_stage_fused(
    stage: &CompiledStage,
    particle_capacity: u32,
    workgroup_size: u32,
) -> ExecutionBlock {
    let particles = ResourceDescriptor {
        id: ResourceTypeId::new(AESTRA_RESOURCE_PARTICLES),
        bytes: 0, // sized by the backend from the particle layout; identity is what matters here
        lifetime: ResourceLifetime::Persistent,
    };
    ExecutionBlock {
        resources: vec![particles],
        ops: vec![ExecutionOp::Compute(ComputeOp {
            name: stage.stage_type.as_str().to_string(),
            program: None,
            entry_point: stage.stage_type.as_str().to_string(),
            accesses: vec![ResourceAccess::read_write(AESTRA_RESOURCE_PARTICLES)],
            dispatch: StagedDispatch {
                x: particle_capacity.div_ceil(workgroup_size.max(1)),
                y: 1,
                z: 1,
            },
            indirect: None,
        })],
        constants: Vec::new(),
        fields: Vec::new(),
        emissions: Vec::new(),
        outputs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompiledStage, Expression, Instruction, ScalarSource, VectorSource};
    use aestra_core::{ModuleId, StageId};

    fn compute(name: &str) -> ExecutionOp {
        ExecutionOp::Compute(ComputeOp {
            name: name.to_string(),
            program: None,
            entry_point: name.to_string(),
            accesses: vec![ResourceAccess::read_write("res.field")],
            dispatch: StagedDispatch { x: 8, y: 1, z: 1 },
            indirect: None,
        })
    }

    fn field_block(ops: Vec<ExecutionOp>) -> ExecutionBlock {
        ExecutionBlock {
            resources: vec![ResourceDescriptor {
                id: ResourceTypeId::new("res.field"),
                bytes: 1024,
                lifetime: ResourceLifetime::Persistent,
            }],
            ops,
            constants: Vec::new(),
            fields: Vec::new(),
            emissions: Vec::new(),
            outputs: Vec::new(),
        }
    }

    #[test]
    fn a_multi_pass_stage_validates_and_executes_in_deterministic_order() {
        // The M6 acceptance workload: Compute A, Barrier, Repeat Compute B x4, Compute C.
        let block = field_block(vec![
            compute("A"),
            ExecutionOp::Barrier,
            ExecutionOp::Repeat {
                policy: RepeatPolicy::FixedCount(4),
                body: vec![compute("B")],
            },
            compute("C"),
        ]);
        block.validate().expect("the block is valid");
        assert_eq!(block.compute_pass_count(), 6, "A + 4xB + C compute passes");

        let trace = execute_reference(&block);
        assert_eq!(
            trace.steps,
            vec![
                "compute:A",
                "barrier",
                "compute:B",
                "compute:B",
                "compute:B",
                "compute:B",
                "compute:C",
            ],
            "repeats expand and the barrier stays in place, deterministically"
        );
    }

    #[test]
    fn validation_rejects_undeclared_resources_zero_dispatch_and_zero_repeat() {
        let undeclared = ExecutionBlock {
            resources: Vec::new(),
            ops: vec![compute("A")],
            constants: Vec::new(),
            fields: Vec::new(),
            emissions: Vec::new(),
            outputs: Vec::new(),
        };
        assert!(matches!(
            undeclared.validate(),
            Err(ExecutionError::UnknownResource(_))
        ));

        let zero_dispatch = field_block(vec![ExecutionOp::Compute(ComputeOp {
            name: "A".to_string(),
            program: None,
            entry_point: "A".to_string(),
            accesses: vec![ResourceAccess::read_write("res.field")],
            dispatch: StagedDispatch { x: 0, y: 1, z: 1 },
            indirect: None,
        })]);
        assert!(matches!(
            zero_dispatch.validate(),
            Err(ExecutionError::EmptyDispatch(_))
        ));

        let zero_repeat = field_block(vec![ExecutionOp::Repeat {
            policy: RepeatPolicy::FixedCount(0),
            body: vec![compute("A")],
        }]);
        assert_eq!(zero_repeat.validate(), Err(ExecutionError::ZeroRepeat));
    }

    #[test]
    fn a_convergent_repeat_traces_its_test_and_holds_only_skippable_ops() {
        let until = |body: Vec<ExecutionOp>, tolerance: f32| ExecutionOp::Repeat {
            policy: RepeatPolicy::UntilConverged {
                residual: ResourceTypeId::new("res.field"),
                tolerance,
                max: 2,
                test_first: false,
            },
            body,
        };
        let block = field_block(vec![until(
            vec![compute("B"), ExecutionOp::Barrier, compute("C")],
            1e-3,
        )]);
        block.validate().expect("compute ops and barriers only");
        assert_eq!(block.compute_pass_count(), 4, "counted at the cap");
        assert_eq!(
            execute_reference(&block).steps,
            vec![
                "compute:B",
                "barrier",
                "compute:C",
                "converged?:res.field",
                "compute:B",
                "barrier",
                "compute:C",
                "converged?:res.field",
            ]
        );

        let copy = ExecutionOp::Copy(CopyOp {
            from: ResourceTypeId::new("res.field"),
            to: ResourceTypeId::new("res.field"),
        });
        for invalid in [
            until(vec![compute("B"), copy], 1e-3),
            until(vec![until(vec![compute("B")], 1e-3)], 1e-3),
            until(vec![compute("B")], f32::NAN),
            until(vec![compute("B")], -1.0),
        ] {
            assert_eq!(
                field_block(vec![invalid]).validate(),
                Err(ExecutionError::InvalidConvergentRepeat)
            );
        }
        let unknown = field_block(vec![ExecutionOp::Repeat {
            policy: RepeatPolicy::UntilConverged {
                residual: ResourceTypeId::new("res.missing"),
                tolerance: 0.0,
                max: 1,
                test_first: false,
            },
            body: vec![compute("B")],
        }]);
        assert!(matches!(
            unknown.validate(),
            Err(ExecutionError::UnknownResource(_))
        ));

        // Tested first too: a repeat that starts converged runs nothing.
        let first = field_block(vec![ExecutionOp::Repeat {
            policy: RepeatPolicy::UntilConverged {
                residual: ResourceTypeId::new("res.field"),
                tolerance: 0.0,
                max: 1,
                test_first: true,
            },
            body: vec![compute("B")],
        }]);
        assert_eq!(
            execute_reference(&first).steps,
            vec!["converged?:res.field", "compute:B", "converged?:res.field"]
        );
    }

    #[test]
    fn an_indirect_op_reads_its_counts_from_a_declared_resource_that_holds_them() {
        let indirect_with = |resource: &str, word: u32, access: ResourceAccess| {
            field_block(vec![ExecutionOp::Compute(ComputeOp {
                name: "A".to_string(),
                program: None,
                entry_point: "A".to_string(),
                accesses: vec![access],
                dispatch: StagedDispatch { x: 8, y: 1, z: 1 },
                indirect: Some(IndirectDispatch {
                    resource: ResourceTypeId::new(resource),
                    word,
                }),
            })])
        };
        let indirect = |resource: &str, word: u32| {
            indirect_with(resource, word, ResourceAccess::read("res.field"))
        };
        // 1024 bytes: the last three words start at word 253.
        indirect("res.field", 253).validate().expect("fits");
        assert_eq!(
            indirect("res.field", 254).validate(),
            Err(ExecutionError::InvalidIndirect("A".into()))
        );
        assert!(matches!(
            indirect("res.missing", 0).validate(),
            Err(ExecutionError::UnknownResource(_))
        ));
        assert_eq!(
            execute_reference(&indirect("res.field", 0)).steps,
            vec!["compute:A"]
        );
        // A dispatch cannot read its counts from a buffer the op writes.
        assert_eq!(
            indirect_with("res.field", 0, ResourceAccess::read_write("res.field")).validate(),
            Err(ExecutionError::InvalidIndirect("A".into()))
        );
    }

    #[test]
    fn host_written_builtins_must_be_declared_persistent_at_their_fixed_size() {
        let mut block = field_block(vec![compute("A")]);
        block.constants = vec![1, 2, 3];
        block.resources.push(block.constants_resource());
        block.resources.push(ExecutionBlock::frame_resource());
        block
            .validate()
            .expect("correctly sized built-ins validate");
        assert_eq!(
            block.binding_of(&ResourceTypeId::new(AESTRA_RESOURCE_FRAME)),
            Some(2),
            "bindings follow declaration order"
        );

        let mut wrong_size = block.clone();
        wrong_size.constants.push(4);
        assert_eq!(
            wrong_size.validate(),
            Err(ExecutionError::InvalidBuiltinResource(ResourceTypeId::new(
                AESTRA_RESOURCE_STAGE_CONSTANTS
            )))
        );
        let mut transient_frame = block;
        transient_frame.resources[2].lifetime = ResourceLifetime::Transient;
        assert!(matches!(
            transient_frame.validate(),
            Err(ExecutionError::InvalidBuiltinResource(_))
        ));
    }

    #[test]
    fn frame_constants_pack_tick_dt_time_and_seed() {
        let frame = FrameConstants::fixed_step(30, 0.5, 9);
        assert_eq!(frame.time, 15.0);
        let words = frame.to_words();
        assert_eq!(words[..4], [30, 0.5f32.to_bits(), 15.0f32.to_bits(), 9]);
        assert_eq!(
            words[4..8],
            [1.0f32.to_bits(), 0, 0, 0],
            "identity placement, row-major"
        );
    }

    fn motion_instruction() -> Instruction {
        Instruction::Motion {
            source: ModuleId::new(),
            gravity: VectorSource::Constant(Expression::Constant([0.0, -9.8, 0.0])),
            drag: ScalarSource::Constant(Expression::Constant(0.5)),
            turbulence: ScalarSource::Constant(Expression::Constant(0.0)),
        }
    }

    #[test]
    fn a_built_in_particle_stage_lowers_fused_to_a_single_compute_pass() {
        // A particle-update stage with several modules must lower to ONE compute op, not one per
        // module — the M6 "do not force each module into a separate dispatch" requirement.
        let stage = CompiledStage {
            id: StageId::for_name("x"),
            stage_type: aestra_core::StageTypeId::new(aestra_core::AESTRA_STAGE_PARTICLE_UPDATE),
            instructions: vec![motion_instruction(), motion_instruction()],
        };
        assert_eq!(stage.instructions.len(), 2, "the stage has two modules");

        let block = lower_stage_fused(&stage, 256, 64);
        block.validate().unwrap();
        let compute_ops = block
            .ops
            .iter()
            .filter(|op| matches!(op, ExecutionOp::Compute(_)))
            .count();
        assert_eq!(
            compute_ops, 1,
            "the whole stage fuses into a single compute pass"
        );
        assert_eq!(block.compute_pass_count(), 1);
    }
}
