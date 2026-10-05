//! Read-only benchmark instrumentation through Bevy 0.19's public buffer bindings.
//! Physical buffer sizes and public asynchronous index demand, NOT whole renderer memory.
use super::*;
use bevy::{
    pbr::{GlobalClusterableObjectMeta, ViewClusterBindings},
    render::{render_resource::BindingResource, sync_world::MainEntity},
};

pub const SCOPE: &str = "Public native buffer.size() observations after render preparation: global clustered-light storage once, per-view cluster index lists and offsets/counts. Includes warmup/startup observations; final in-flight render frames may be absent. Main-world cluster observations additionally retain public native asynchronous index-demand acknowledgments and grid dimensions, not frame-paired with buffers/timestamps. Z-slice sizes are null because their public type is in Bevy 0.19's private gpu module. Excludes private Z-slice/scratchpad/metadata/staging buffers, old in-flight allocations, textures and allocator overhead. Not total resident memory or a hard cap. Missing bindings/counters are null, not zero. Native resize logs remain a separate overflow/growth gate.";

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
}
#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    pub sequence: u64,
    pub tick: Tick,
    pub global_light_bytes: Option<u64>,
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
    fn push(&self, tick: Tick, global_light_bytes: Option<u64>, views: Vec<ViewBuffers>) {
        let mut results = self.0.lock().unwrap();
        results.sequence += 1;
        let sequence = results.sequence;
        if results.pending.len() == 4 {
            results.pending.remove(0);
            results.overwritten += 1;
        }
        results.pending.push(Observation {
            sequence,
            tick,
            global_light_bytes,
            views,
        });
    }
}
fn allocated_bytes(binding: Option<BindingResource<'_>>) -> Option<u64> {
    match binding {
        Some(BindingResource::Buffer(binding)) => Some(binding.buffer.size()),
        _ => None,
    }
}
pub fn observe(
    global: Option<Res<GlobalClusterableObjectMeta>>,
    views: Query<(&MainEntity, &ViewClusterBindings)>,
    tick: Res<Tick>,
    mailbox: Res<Mailbox>,
) {
    let global_light_bytes = global
        .as_ref()
        .and_then(|g| allocated_bytes(g.gpu_clustered_lights.binding()));
    let mut views = views
        .iter()
        .map(|(entity, bindings)| ViewBuffers {
            main_entity: entity.id().to_string(),
            z_slice_bytes: None,
            index_bytes: allocated_bytes(bindings.clusterable_object_index_lists_binding()),
            offsets_counts_bytes: allocated_bytes(bindings.offsets_and_counts_binding()),
        })
        .collect::<Vec<_>>();
    views.sort_by(|a, b| a.main_entity.cmp(&b.main_entity));
    mailbox.push(*tick, global_light_bytes, views);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observations_preserve_unknowns_origin_and_bounded_delivery() {
        let mailbox = Mailbox::default();
        for index in 0..7 {
            mailbox.push(
                Tick {
                    index,
                    measured: index >= 5,
                    host_elapsed_seconds: 0.0,
                },
                None,
                vec![ViewBuffers {
                    main_entity: "camera".into(),
                    z_slice_bytes: Some(12),
                    index_bytes: None,
                    offsets_counts_bytes: Some(32),
                }],
            );
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
