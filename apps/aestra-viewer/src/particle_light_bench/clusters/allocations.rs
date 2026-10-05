//! Opt-in backend suballocator census. No GPU waits, maps or retained GPU handles.
use bevy::{prelude::*, render::renderer::RenderDevice};
use serde::Serialize;

pub const PERIOD: u64 = 60;
pub const LIMIT: u64 = 64;
pub const SCOPE: &str = "Opt-in wgpu device generate_allocator_report(), once per 60 render observations, at most 64 snapshots. Backend suballocator live allocations and reserved blocks include resources retained for pending GPU work when the backend reports them. Whole device, not cluster-only, process VRAM, a hard budget or a complete in-flight fence/creation trace. Names/offsets/block order are not stable allocation identities; unlabeled buffers cannot be attributed to private Z-slices/scratchpad. No report means unavailable, never zero. CPU allocator locks/census can perturb pacing, so these are allocation qualification runs, not matched cost evidence. No added GPU readback, wait or retained resource handle.";

#[derive(Resource)]
pub struct Settings(pub bool);
#[derive(Clone, Debug, Serialize)]
pub struct Allocation {
    pub name: String,
    pub offset: u64,
    pub size: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Block {
    pub size: u64,
    pub allocation_range: [usize; 2],
}
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub total_allocated_bytes: u64,
    pub total_reserved_bytes: u64,
    pub allocations: Vec<Allocation>,
    pub blocks: Vec<Block>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Sample {
    pub cpu_ms: f64,
    pub report: Option<Snapshot>,
}
pub fn sample(device: &RenderDevice) -> Sample {
    let started = std::time::Instant::now();
    let report = device
        .wgpu_device()
        .generate_allocator_report()
        .map(|r| Snapshot {
            total_allocated_bytes: r.total_allocated_bytes,
            total_reserved_bytes: r.total_reserved_bytes,
            allocations: r
                .allocations
                .into_iter()
                .map(|a| Allocation {
                    name: a.name,
                    offset: a.offset,
                    size: a.size,
                })
                .collect(),
            blocks: r
                .blocks
                .into_iter()
                .map(|b| Block {
                    size: b.size,
                    allocation_range: [b.allocations.start, b.allocations.end],
                })
                .collect(),
        });
    Sample {
        cpu_ms: started.elapsed().as_secs_f64() * 1000.0,
        report,
    }
}

pub fn scheduled(sequence: u64) -> bool {
    sequence > 0 && (sequence - 1).is_multiple_of(PERIOD)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_and_unknown_are_explicit_without_gpu_work() {
        assert!(!scheduled(0));
        assert!(scheduled(1) && scheduled(61));
        assert!(!scheduled(60) && !scheduled(62));
        assert_eq!(
            serde_json::to_value(Sample {
                cpu_ms: 0.0,
                report: None
            })
            .unwrap()["report"],
            serde_json::Value::Null
        );
    }
}
