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

use crate::{CompiledStage, StagedDispatch};
use aestra_core::ResourceTypeId;

/// The built-in resource id for an emitter's particle buffer — what the standard fused particle stages
/// read and write.
pub const AESTRA_RESOURCE_PARTICLES: &str = "aestra.resource.particles";

/// The built-in resource holding an effect instance's host binding snapshots, packed per the
/// `aestra_gpu` host-binding ABI (host bindings HB6). Host-written once per tick, read-only to stages:
/// a plugin stage that reads bound objects declares it and a `Read` access on its compute ops.
pub const AESTRA_RESOURCE_HOST_BINDINGS: &str = "aestra.resource.host_bindings";

/// The domain of host-supplied inputs (host bindings HB6).
pub const AESTRA_DOMAIN_HOST_INPUT: &str = "aestra.domain.host_input";

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
    pub entry_point: String,
    pub accesses: Vec<ResourceAccess>,
    pub dispatch: StagedDispatch,
}

/// A copy between two resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyOp {
    pub from: ResourceTypeId,
    pub to: ResourceTypeId,
}

/// How many times a [`ExecutionOp::Repeat`] body runs (extensible-stages M6). An enum so a future
/// "until converged" policy can be added without changing call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatPolicy {
    /// Run the body exactly `n` times (a fixed solver-iteration count).
    FixedCount(u32),
}

impl RepeatPolicy {
    pub fn count(self) -> u32 {
        match self {
            Self::FixedCount(n) => n,
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

/// The ordered execution plan of one stage (extensible-stages M6): declared resources plus the ops
/// that run over them, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecutionBlock {
    pub resources: Vec<ResourceDescriptor>,
    pub ops: Vec<ExecutionOp>,
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
        }
        validate_ops(&self.ops, &declared)
    }

    /// The number of compute passes a run of this block performs, with repeats expanded — the count of
    /// GPU dispatches (and of pass-level timestamp intervals) the native backend will issue.
    pub fn compute_pass_count(&self) -> u32 {
        count_compute(&self.ops)
    }
}

fn validate_ops(
    ops: &[ExecutionOp],
    declared: &std::collections::BTreeSet<ResourceTypeId>,
) -> Result<(), ExecutionError> {
    for op in ops {
        match op {
            ExecutionOp::Compute(compute) => {
                if !compute.dispatch.is_nonzero() {
                    return Err(ExecutionError::EmptyDispatch(compute.name.clone()));
                }
                for access in &compute.accesses {
                    if !declared.contains(&access.resource) {
                        return Err(ExecutionError::UnknownResource(access.resource.clone()));
                    }
                }
            }
            ExecutionOp::Copy(copy) => {
                for id in [&copy.from, &copy.to] {
                    if !declared.contains(id) {
                        return Err(ExecutionError::UnknownResource(id.clone()));
                    }
                }
            }
            ExecutionOp::Barrier => {}
            ExecutionOp::Repeat { policy, body } => {
                if policy.count() == 0 {
                    return Err(ExecutionError::ZeroRepeat);
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
                for _ in 0..policy.count() {
                    trace_ops(body, steps);
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
            entry_point: stage.stage_type.as_str().to_string(),
            accesses: vec![ResourceAccess::read_write(AESTRA_RESOURCE_PARTICLES)],
            dispatch: StagedDispatch {
                x: particle_capacity.div_ceil(workgroup_size.max(1)),
                y: 1,
                z: 1,
            },
        })],
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
            entry_point: name.to_string(),
            accesses: vec![ResourceAccess::read_write("res.field")],
            dispatch: StagedDispatch { x: 8, y: 1, z: 1 },
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
        };
        assert!(matches!(
            undeclared.validate(),
            Err(ExecutionError::UnknownResource(_))
        ));

        let zero_dispatch = field_block(vec![ExecutionOp::Compute(ComputeOp {
            name: "A".to_string(),
            entry_point: "A".to_string(),
            accesses: vec![ResourceAccess::read_write("res.field")],
            dispatch: StagedDispatch { x: 0, y: 1, z: 1 },
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
