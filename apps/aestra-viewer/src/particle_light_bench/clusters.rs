//! Read-only benchmark instrumentation through Bevy 0.19's public buffer bindings.
//! Physical buffer sizes and public asynchronous index demand, NOT whole renderer memory.
use super::*;
use bevy::{
    pbr::{GlobalClusterableObjectMeta, ViewClusterBindings},
    render::{render_resource::BindingResource, renderer::RenderDevice, sync_world::MainEntity},
};
use std::hash::{Hash, Hasher};
pub mod allocations;

pub const SCOPE: &str = "Public native buffer.size() observations after render preparation: global clustered-light storage once, per-view cluster index lists and offsets/counts. Hashed wgpu handle identities expose observed replacements, not a complete create/free/fence trace; no GPU handle clones are retained. Includes warmup/startup observations; final in-flight render frames may be absent. Main-world cluster observations additionally retain public native asynchronous index-demand acknowledgments and grid dimensions, not frame-paired with buffers/timestamps. Z-slice sizes are null because their public type is in Bevy 0.19's private gpu module. Excludes private Z-slice/scratchpad/metadata/staging buffers, old in-flight allocations, textures and allocator overhead. Not total resident memory or a hard cap. Missing bindings/counters are null, not zero. Native resize logs remain a separate overflow/growth gate. Opt-in backend allocator snapshots have their own distinct allocator_scope.";

#[derive(Debug, Serialize)]
pub struct MainView {
    pub main_entity: String,
    pub dimensions: [u32; 3],
    pub last_native_index_demand: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct MainObservation {
    pub sample: usize,
    pub gpu_clustering_enabled: bool,
    pub adaptive_index_target: usize,
    pub views: Vec<MainView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ViewBuffers {
    pub main_entity: String,
    pub z_slice_bytes: Option<u64>,
    pub index_bytes: Option<u64>,
    pub offsets_counts_bytes: Option<u64>,
    pub index_buffer_fingerprint: Option<String>,
    pub offsets_counts_buffer_fingerprint: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    pub sequence: u64,
    pub tick: Tick,
    pub global_light_bytes: Option<u64>,
    pub global_light_buffer_fingerprint: Option<String>,
    pub allocation_sample: Option<allocations::Sample>,
    pub allocation_sampling_exhausted: bool,
    pub views: Vec<ViewBuffers>,
}
#[derive(Default)]
struct Results {
    sequence: u64,
    pending: Vec<Observation>,
    overwritten: u64,
}
#[derive(Resource, Default, Clone)]
pub struct Mailbox(Arc<Mutex<Results>>);
impl Mailbox {
    pub fn take(&self) -> (Vec<Observation>, u64) {
        let mut results = self.0.lock().unwrap();
        (std::mem::take(&mut results.pending), results.overwritten)
    }
    fn next_sequence(&self) -> u64 {
        self.0.lock().unwrap().sequence + 1
    }
    fn push(&self, mut observation: Observation) {
        let mut results = self.0.lock().unwrap();
        results.sequence += 1;
        let sequence = results.sequence;
        if results.pending.len() == 4 {
            results.pending.remove(0);
            results.overwritten += 1;
        }
        observation.sequence = sequence;
        results.pending.push(observation);
    }
}
// Hash the public wgpu handle identity, not its label/size or a retained clone.
// Fingerprints detect observed replacements, not hidden create/free operations
// between observations or a complete allocation-generation/fence history.
fn binding_info(binding: Option<BindingResource<'_>>) -> (Option<u64>, Option<String>) {
    match binding {
        Some(BindingResource::Buffer(binding)) => {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            binding.buffer.hash(&mut hasher);
            (
                Some(binding.buffer.size()),
                Some(format!("{:016x}", hasher.finish())),
            )
        }
        _ => (None, None),
    }
}
pub fn observe(
    global: Option<Res<GlobalClusterableObjectMeta>>,
    views: Query<(&MainEntity, &ViewClusterBindings)>,
    tick: Res<Tick>,
    mailbox: Res<Mailbox>,
    device: Res<RenderDevice>,
    allocations: Option<Res<allocations::Settings>>,
    mut census_count: Local<u64>,
) {
    let (global_light_bytes, global_light_buffer_fingerprint) =
        global.as_ref().map_or((None, None), |g| {
            binding_info(g.gpu_clustered_lights.binding())
        });
    let mut views = views
        .iter()
        .map(|(entity, bindings)| {
            let (index_bytes, index_buffer_fingerprint) =
                binding_info(bindings.clusterable_object_index_lists_binding());
            let (offsets_counts_bytes, offsets_counts_buffer_fingerprint) =
                binding_info(bindings.offsets_and_counts_binding());
            ViewBuffers {
                main_entity: entity.id().to_string(),
                z_slice_bytes: None,
                index_bytes,
                offsets_counts_bytes,
                index_buffer_fingerprint,
                offsets_counts_buffer_fingerprint,
            }
        })
        .collect::<Vec<_>>();
    views.sort_by(|a, b| a.main_entity.cmp(&b.main_entity));
    let scheduled =
        allocations.is_some_and(|a| a.0) && allocations::scheduled(mailbox.next_sequence());
    let exhausted = scheduled && *census_count >= allocations::LIMIT;
    let allocation_sample = (scheduled && !exhausted).then(|| {
        *census_count += 1;
        allocations::sample(&device)
    });
    mailbox.push(Observation {
        sequence: 0,
        tick: *tick,
        global_light_bytes,
        global_light_buffer_fingerprint,
        views,
        allocation_sample,
        allocation_sampling_exhausted: exhausted,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observations_preserve_unknowns_origin_and_bounded_delivery() {
        let mailbox = Mailbox::default();
        for index in 0..7 {
            mailbox.push(Observation {
                sequence: 0,
                tick: Tick {
                    index,
                    measured: index >= 5,
                    host_elapsed_seconds: 0.0,
                },
                global_light_bytes: None,
                global_light_buffer_fingerprint: None,
                allocation_sample: None,
                allocation_sampling_exhausted: false,
                views: vec![ViewBuffers {
                    main_entity: "camera".into(),
                    z_slice_bytes: Some(12),
                    index_bytes: None,
                    offsets_counts_bytes: Some(32),
                    index_buffer_fingerprint: None,
                    offsets_counts_buffer_fingerprint: None,
                }],
            });
        }
        let (samples, lost) = mailbox.take();
        assert_eq!(lost, 3);
        assert_eq!(
            samples.iter().map(|s| s.sequence).collect::<Vec<_>>(),
            [4, 5, 6, 7]
        );
        assert_eq!(
            samples.iter().map(|s| s.tick.index).collect::<Vec<_>>(),
            [3, 4, 5, 6]
        );
        assert_eq!(samples[0].global_light_bytes, None);
        assert_eq!(samples[0].views[0].index_bytes, None);
        assert_eq!(samples[0].views[0].z_slice_bytes, Some(12));
        assert_eq!(mailbox.take().0.len(), 0);
    }
}
