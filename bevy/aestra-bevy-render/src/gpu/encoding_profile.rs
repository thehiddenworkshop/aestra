//! Optional CPU allocation/encoding call-site accounting, local to the simulation thread.
//! These are API requests, not driver allocation traces or GPU elapsed times.
use std::{cell::RefCell, time::Instant};

#[derive(Clone, Debug, Default)]
pub struct GpuEncodingOperation {
    pub name: &'static str,
    pub calls: u64,
    pub cpu_ns: u64,
    pub buffers: u64,
    pub buffer_bytes: u64,
    /// Bytes passed to create-buffer-with-data, not measured transfer traffic.
    pub initialized_bytes: u64,
    pub bind_groups: u64,
    pub dispatches: u64,
    pub copy_bytes: u64,
    pub queue_write_bytes: u64,
}

#[derive(Clone, Copy)]
pub(crate) enum Operation {
    EventList,
    TickParams,
    PresentParams,
    StatefulGroup,
    Physics,
    TrailUpload,
    EventGather,
    EventSpawn,
    EventGatherPrepare,
    EventSpawnPrepare,
    ParameterPage,
    ParameterDestination,
    ParameterCopy,
    ParameterUpload,
}
const NAMES: [&str; 14] = [
    "event_list",
    "tick_params",
    "present_params",
    "stateful_bind_group",
    "physics_upload",
    "trail_observation_upload",
    "event_gather",
    "event_spawn",
    "event_gather_prepare",
    "event_spawn_prepare",
    "parameter_page",
    "parameter_destination",
    "parameter_copy",
    "parameter_upload",
];
thread_local! {
    static ACTIVE: RefCell<Option<[GpuEncodingOperation; 14]>> = const { RefCell::new(None) };
}

pub(super) fn begin() {
    ACTIVE.with(|active| {
        assert!(
            active.borrow().is_none(),
            "simulation profiling must not nest"
        );
        *active.borrow_mut() = Some(std::array::from_fn(|i| GpuEncodingOperation {
            name: NAMES[i],
            ..Default::default()
        }));
    });
}
pub(super) fn finish() -> Vec<GpuEncodingOperation> {
    ACTIVE.with(|active| {
        active
            .borrow_mut()
            .take()
            .unwrap()
            .into_iter()
            .filter(|s| s.calls != 0)
            .collect()
    })
}

/// Call-site scope. Categories do not nest in the instrumented production path.
pub(crate) fn scope(
    op: Operation,
    buffers: u64,
    bytes: u64,
    initialized: u64,
    groups: u64,
    dispatches: u64,
) -> Scope {
    let started = ACTIVE
        .with(|active| active.borrow().is_some())
        .then(Instant::now);
    Scope {
        op,
        started,
        buffers,
        bytes,
        initialized,
        groups,
        dispatches,
        copy_bytes: 0,
        queue_write_bytes: 0,
    }
}
pub(super) fn transfer(op: Operation, copy_bytes: u64, queue_write_bytes: u64) -> Scope {
    let mut scope = scope(op, 0, 0, 0, 0, 0);
    scope.copy_bytes = copy_bytes;
    scope.queue_write_bytes = queue_write_bytes;
    scope
}
pub(crate) struct Scope {
    op: Operation,
    started: Option<Instant>,
    buffers: u64,
    bytes: u64,
    initialized: u64,
    groups: u64,
    dispatches: u64,
    copy_bytes: u64,
    queue_write_bytes: u64,
}
impl Drop for Scope {
    fn drop(&mut self) {
        let Some(start) = self.started else {
            return;
        };
        let ns = start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        ACTIVE.with(|active| {
            if let Some(stats) = active.borrow_mut().as_mut() {
                let s = &mut stats[self.op as usize];
                s.calls += 1;
                s.cpu_ns += ns;
                s.buffers += self.buffers;
                s.buffer_bytes += self.bytes;
                s.initialized_bytes += self.initialized;
                s.bind_groups += self.groups;
                s.dispatches += self.dispatches;
                s.copy_bytes += self.copy_bytes;
                s.queue_write_bytes += self.queue_write_bytes;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accounting_is_opt_in_and_each_window_retires_its_counts() {
        drop(scope(Operation::EventList, 1, 48, 48, 0, 0));
        begin();
        drop(scope(Operation::EventList, 1, 48, 48, 0, 0));
        drop(scope(Operation::EventList, 1, 16, 0, 0, 0));
        let stats = finish();
        assert_eq!(stats.len(), 1);
        assert_eq!(
            (
                stats[0].calls,
                stats[0].buffers,
                stats[0].buffer_bytes,
                stats[0].initialized_bytes
            ),
            (2, 2, 64, 48)
        );
        begin();
        assert!(finish().is_empty());
    }
}
