//! The generic staged simulation plan (hybrid roadmap M13).
//!
//! `SimulationClass::Staged` is stateful *plus* ordered, possibly-iterated compute passes over shared
//! grid/field resources — the shape a grid solver (diffusion, advection, reaction-diffusion, and later
//! fluids) needs. This module is the portable, engine-independent **description** of such a
//! computation: named resources (some double-buffered, some transient scratch), and an ordered list of
//! passes that read and write them, each with a dispatch shape and an iteration count. A GPU executor
//! (in the render backend / conformance harness) consumes a [`StagedPlan`] to allocate the resources
//! and run the passes; nothing here depends on any GPU API.
//!
//! The first validation workload is 2D diffusion (see [`diffuse_2d_step`]) — a single pass iterated
//! over a ping-ponged grid — deterministic and trig-free so the GPU executor can be conformance-checked
//! against this CPU reference bit-for-bit.

/// Whether a staged resource persists across ticks — and so is checkpointed and restorable — or is
/// transient scratch that lives only within a tick's pass sequence (hybrid roadmap M13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagedResourceLifetime {
    /// Carried across ticks: part of the simulation state, snapshotted into checkpoints so a backward
    /// seek can restore and replay (the ping-pong "current" buffer is the one checkpointed).
    Persistent,
    /// Scratch that only needs to live within one tick's pass sequence; never checkpointed.
    Transient,
}

/// A named resource a staged plan operates on: a typed GPU buffer of `bytes` bytes (hybrid roadmap
/// M13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedResource {
    pub name: String,
    /// Byte size of one buffer of this resource.
    pub bytes: u64,
    /// Double-buffered: a pass that writes it reads the *front* buffer and writes the *back*, and the
    /// executor swaps them after the pass, so successive iterations alternate (ping-pong). A persistent
    /// ping-pong resource checkpoints its current front buffer.
    pub ping_pong: bool,
    pub lifetime: StagedResourceLifetime,
}

/// The workgroup dispatch shape of a pass (hybrid roadmap M13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagedDispatch {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

impl StagedDispatch {
    pub fn is_nonzero(&self) -> bool {
        self.x > 0 && self.y > 0 && self.z > 0
    }
}

/// One compute pass of a staged plan (hybrid roadmap M13): a shader entry point, the resources it reads
/// and writes (by name), a dispatch shape, and how many times to iterate it. Passes run in plan order;
/// each pass is a separate compute pass, so a read-after-write on a shared resource between passes (and
/// between iterations of one pass) is a correctly-ordered barrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPass {
    pub name: String,
    pub entry_point: String,
    /// Resources read this pass, bound read-only, in order.
    pub reads: Vec<String>,
    /// Resources written this pass, bound read-write, in order. A ping-pong resource that is both read
    /// and written alternates front/back per iteration.
    pub writes: Vec<String>,
    pub dispatch: StagedDispatch,
    /// Times to repeat this pass (>= 1). Ping-pong resources swap between iterations, so N iterations
    /// of a diffusion pass advance the field N steps.
    pub iterations: u32,
}

/// A generic staged simulation plan (hybrid roadmap M13): the resources and the ordered pass graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPlan {
    pub resources: Vec<StagedResource>,
    pub passes: Vec<StagedPass>,
}

/// Why a [`StagedPlan`] is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedPlanError {
    /// Two resources share a name.
    DuplicateResource(String),
    /// A pass references a resource that no resource declares.
    UnknownResource { pass: String, resource: String },
    /// A resource has zero size.
    EmptyResource(String),
    /// A pass has a zero dispatch shape.
    EmptyDispatch(String),
    /// A pass has zero iterations.
    ZeroIterations(String),
}

impl core::fmt::Display for StagedPlanError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicateResource(name) => write!(f, "duplicate staged resource '{name}'"),
            Self::UnknownResource { pass, resource } => {
                write!(f, "pass '{pass}' references unknown resource '{resource}'")
            }
            Self::EmptyResource(name) => write!(f, "staged resource '{name}' has zero size"),
            Self::EmptyDispatch(name) => write!(f, "pass '{name}' has a zero dispatch shape"),
            Self::ZeroIterations(name) => write!(f, "pass '{name}' has zero iterations"),
        }
    }
}

impl std::error::Error for StagedPlanError {}

impl StagedPlan {
    /// Validates the plan: unique non-empty resources, every pass reference resolves, and every pass
    /// has a non-zero dispatch and at least one iteration. Ordering is the pass vector's order, so a
    /// valid plan executes deterministically.
    pub fn validate(&self) -> Result<(), StagedPlanError> {
        let mut seen = std::collections::BTreeSet::new();
        for resource in &self.resources {
            if resource.bytes == 0 {
                return Err(StagedPlanError::EmptyResource(resource.name.clone()));
            }
            if !seen.insert(resource.name.as_str()) {
                return Err(StagedPlanError::DuplicateResource(resource.name.clone()));
            }
        }
        for pass in &self.passes {
            if !pass.dispatch.is_nonzero() {
                return Err(StagedPlanError::EmptyDispatch(pass.name.clone()));
            }
            if pass.iterations == 0 {
                return Err(StagedPlanError::ZeroIterations(pass.name.clone()));
            }
            for resource in pass.reads.iter().chain(pass.writes.iter()) {
                if !seen.contains(resource.as_str()) {
                    return Err(StagedPlanError::UnknownResource {
                        pass: pass.name.clone(),
                        resource: resource.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn resource(&self, name: &str) -> Option<&StagedResource> {
        self.resources.iter().find(|resource| resource.name == name)
    }

    /// The total number of pass executions (summing iterations), i.e. the number of ordered compute
    /// dispatches — the count of pass-level GPU timestamp intervals to allocate.
    pub fn total_dispatches(&self) -> u32 {
        self.passes.iter().map(|pass| pass.iterations).sum()
    }

    /// The persistent resources — the ones a checkpoint must snapshot so a staged effect can
    /// restore/replay (hybrid roadmap M13). Transient scratch is excluded.
    pub fn checkpointed_resources(&self) -> impl Iterator<Item = &StagedResource> {
        self.resources
            .iter()
            .filter(|resource| resource.lifetime == StagedResourceLifetime::Persistent)
    }
}

/// One explicit (forward-Euler / Jacobi) step of 2D diffusion on a periodic (wrap-around) grid — the
/// M13 staged-infrastructure validation workload. `output[i] = input[i] + rate * (Σ 4-neighbours −
/// 4·input[i])`; stable for `rate <= 0.25`. Uses only `+ - *`, so the GPU kernel reproduces it
/// bit-for-bit, and it is fully deterministic from `(input, rate)`.
///
/// `output` must be `width * height` long, matching `input`. Neighbours wrap at the edges, so there is
/// no boundary special-casing to diverge on.
pub fn diffuse_2d_step(input: &[f32], output: &mut [f32], width: usize, height: usize, rate: f32) {
    debug_assert_eq!(input.len(), width * height);
    debug_assert_eq!(output.len(), width * height);
    for y in 0..height {
        let up = if y == 0 { height - 1 } else { y - 1 };
        let down = if y == height - 1 { 0 } else { y + 1 };
        for x in 0..width {
            let left = if x == 0 { width - 1 } else { x - 1 };
            let right = if x == width - 1 { 0 } else { x + 1 };
            let center = input[y * width + x];
            let neighbours = input[y * width + left]
                + input[y * width + right]
                + input[up * width + x]
                + input[down * width + x];
            output[y * width + x] = center + rate * (neighbours - 4.0 * center);
        }
    }
}

/// Runs `steps` of [`diffuse_2d_step`], ping-ponging two buffers, and returns the final field — the CPU
/// reference the GPU staged executor is conformance-checked against (hybrid roadmap M13).
pub fn diffuse_2d(initial: &[f32], width: usize, height: usize, rate: f32, steps: u32) -> Vec<f32> {
    let mut front = initial.to_vec();
    let mut back = vec![0.0_f32; width * height];
    for _ in 0..steps {
        diffuse_2d_step(&front, &mut back, width, height, rate);
        std::mem::swap(&mut front, &mut back);
    }
    front
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical 2D diffusion validation plan (hybrid roadmap M13): one persistent ping-pong grid,
    /// one diffusion pass iterated `steps` times over an `width`×`height` grid with 8×8 workgroups.
    fn diffusion_plan(width: u32, height: u32, steps: u32) -> StagedPlan {
        StagedPlan {
            resources: vec![StagedResource {
                name: "grid".to_string(),
                bytes: u64::from(width) * u64::from(height) * 4,
                ping_pong: true,
                lifetime: StagedResourceLifetime::Persistent,
            }],
            passes: vec![StagedPass {
                name: "diffuse".to_string(),
                entry_point: "diffuse".to_string(),
                reads: vec!["grid".to_string()],
                writes: vec!["grid".to_string()],
                dispatch: StagedDispatch {
                    x: width.div_ceil(8),
                    y: height.div_ceil(8),
                    z: 1,
                },
                iterations: steps,
            }],
        }
    }

    #[test]
    fn the_diffusion_plan_is_valid_and_deterministic() {
        let plan = diffusion_plan(64, 64, 30);
        plan.validate().expect("diffusion plan is valid");
        assert_eq!(plan.total_dispatches(), 30, "one pass iterated 30 times");
        assert_eq!(
            plan.checkpointed_resources().count(),
            1,
            "the persistent grid is checkpointed"
        );
    }

    #[test]
    fn plan_validation_rejects_dangling_references_and_degenerate_passes() {
        let mut plan = diffusion_plan(8, 8, 4);
        plan.passes[0].reads = vec!["missing".to_string()];
        assert_eq!(
            plan.validate(),
            Err(StagedPlanError::UnknownResource {
                pass: "diffuse".to_string(),
                resource: "missing".to_string(),
            })
        );

        let mut zero_iter = diffusion_plan(8, 8, 4);
        zero_iter.passes[0].iterations = 0;
        assert_eq!(
            zero_iter.validate(),
            Err(StagedPlanError::ZeroIterations("diffuse".to_string()))
        );

        let mut dup = diffusion_plan(8, 8, 4);
        dup.resources.push(dup.resources[0].clone());
        assert_eq!(
            dup.validate(),
            Err(StagedPlanError::DuplicateResource("grid".to_string()))
        );
    }

    #[test]
    fn diffusion_conserves_total_and_relaxes_toward_the_mean() {
        // Diffusion on a periodic grid conserves the sum (no flux leaves) and reduces variance — a hot
        // spike spreads out. A deterministic, trig-free reference for the GPU executor to match.
        let (w, h) = (16, 16);
        let mut initial = vec![0.0_f32; w * h];
        initial[h / 2 * w + w / 2] = 100.0; // a single hot cell
        let sum_initial: f32 = initial.iter().sum();

        let result = diffuse_2d(&initial, w, h, 0.2, 50);
        let sum_result: f32 = result.iter().sum();

        assert!(
            (sum_initial - sum_result).abs() < 1e-2,
            "diffusion conserves the total ({sum_initial} vs {sum_result})"
        );
        let peak = result.iter().cloned().fold(f32::MIN, f32::max);
        assert!(peak < 100.0, "the spike relaxes (peak {peak} < 100)");
        assert!(peak > 0.0, "but heat remains");

        // Fully deterministic: same inputs, same output.
        let again = diffuse_2d(&initial, w, h, 0.2, 50);
        assert_eq!(result, again, "diffusion is deterministic");
    }

    #[test]
    fn ping_pong_checkpoint_and_replay_reproduces_uninterrupted_diffusion() {
        // The CPU analogue of the staged checkpoint/replay acceptance: the field after `a + b` steps
        // equals snapshotting at `a` and replaying `b` more — the ping-pong state survives a checkpoint.
        let (w, h) = (24, 24);
        let mut initial = vec![0.0_f32; w * h];
        for (i, cell) in initial.iter_mut().enumerate() {
            *cell = (i % 7) as f32; // a deterministic non-uniform field
        }
        let uninterrupted = diffuse_2d(&initial, w, h, 0.24, 40);
        let checkpoint = diffuse_2d(&initial, w, h, 0.24, 25); // snapshot at step 25
        let replayed = diffuse_2d(&checkpoint, w, h, 0.24, 15); // replay the remaining 15
        assert_eq!(
            uninterrupted, replayed,
            "checkpoint at 25 + replay 15 == uninterrupted 40"
        );
    }
}
