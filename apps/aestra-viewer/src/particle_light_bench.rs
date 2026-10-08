//! F7D2B benchmark instrumentation, not scene-light realization. Only sixteen
//! counter bytes cross the CPU boundary; three reusable async staging slots.
use aestra_bevy::{
    EffectAsset,
    gpu::particle_lights::{AestraParticleLightSettings, GpuSelectedParticleLights},
};
use bevy::{
    ecs::system::SystemState,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        diagnostic::{RecordDiagnostics, begin_diagnostics_frame, resolve_encoder},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::{Buffer, BufferDescriptor, BufferUsages, MapMode},
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue},
    },
};
use serde::Serialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub mod clusters;

/// Host-only preallocation for the current 96/48/24-light workload matrix.
/// These are initial capacities, not hard limits or a total GPU-memory budget.
/// Bevy may still grow them for other scenes/cameras/caps. Shared by control and
/// enabled runs to avoid first-burst overflow/growth skewing the comparison.
pub fn cluster_capacities(tier: &str) -> [usize; 2] {
    match tier {
        "high" => [4096, 524_288],
        "medium" => [2048, 262_144],
        _ => [1024, 131_072],
    }
}

pub fn prime_clusters(
    config: Res<crate::ViewerConfig>,
    mut clusters: ResMut<bevy::light::cluster::GlobalClusterSettings>,
) {
    configure_clusters(&config.tier.name, &mut clusters);
}

fn configure_clusters(tier: &str, clusters: &mut bevy::light::cluster::GlobalClusterSettings) {
    if let Some(settings) = &mut clusters.gpu_clustering {
        let [slices, indices] = cluster_capacities(tier);
        settings.initial_z_slice_list_capacity = slices;
        settings.initial_index_list_capacity = indices;
        // Bevy's independent default adaptive-grid target otherwise remains
        // at MAX_INDICES even after GPU storage preallocation was increased.
        // Align the host target for both adapter-on and adapter-off controls.
        clusters.view_cluster_bindings_max_indices = indices;
    }
}

/// Deliberately explicit benchmark fixture. Materials, renderer/history budgets,
/// emission, events, transforms and show choreography are not altered.
/// F7F2 deliberately lets a sparse low-tier output use the global 24-slot ceiling;
/// this is a fixture authoring choice, not a host override of authored limits.
pub fn add_outputs(effect: &mut EffectAsset) {
    use aestra_bevy::{
        Curve, CurveId, CurveKey, ParticlePointLightProperties, SceneOutputId, SceneOutputInstance,
    };
    for emitter in &mut effect.emitters {
        let name = emitter.name.to_ascii_lowercase();
        if name.contains("smoke") || !emitter.scene_outputs.is_empty() {
            continue;
        }
        let mut light = ParticlePointLightProperties::new(1500.0, 12.0);
        light.intensity_curve = Curve::new(vec![
            CurveKey::new(0.0, 1500.0),
            CurveKey::new(0.8, 600.0),
            CurveKey::new(1.0, 0.0),
        ]);
        light.intensity_curve.id =
            CurveId::from_u128(emitter.id.as_uuid().as_u128() ^ (0xf7d2_b001_u128 << 96));
        light.range_curve.id =
            CurveId::from_u128(emitter.id.as_uuid().as_u128() ^ (0xf7d2_b002_u128 << 96));
        light.max_lights_by_quality = [
            ("high".into(), 32),
            ("medium".into(), 16),
            ("low".into(), 24),
        ]
        .into();
        light.priority = u32::from(name.contains("flash"));
        let mut output = SceneOutputInstance::particle_point_light(light);
        output.id =
            SceneOutputId::from_u128(emitter.id.as_uuid().as_u128() ^ (0xf7d2_b000_u128 << 96));
        emitter.scene_outputs.push(output);
    }
}

#[derive(Resource, Clone, Copy, Default, ExtractResource, Debug, Serialize)]
pub struct Tick {
    pub index: u64,
    pub measured: bool,
    pub host_elapsed_seconds: f32,
}
#[derive(Resource)]
pub struct Start {
    pub ready: bool,
    waiting_since: std::time::Instant,
}
fn start(
    mut start: ResMut<Start>,
    readiness: Res<crate::CaptureRenderReadiness>,
    mut players: Query<&mut aestra_bevy::EffectPlayer>,
    mut exit: MessageWriter<AppExit>,
) {
    if start.ready {
        return;
    }
    if !readiness.ready {
        if start.waiting_since.elapsed() > std::time::Duration::from_secs(60) {
            error!(
                "particle-light benchmark pipeline startup timed out: {}",
                readiness.detail
            );
            exit.write(AppExit::error());
        }
        return;
    }
    for mut player in &mut players {
        player.restart();
        player.playing = true;
    }
    start.ready = true;
}
#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub tick: Tick,
    pub sequence: Option<u64>,
    pub source_runs: usize,
    pub selected_capacity: u32,
    pub scratch_bytes: Option<u64>,
    pub reserved_bytes: Option<u64>,
    pub rejected_outputs: u32,
    pub counters: Option<[u32; 4]>,
    pub status: String,
}
#[derive(Default)]
struct Results {
    pending: Vec<Observation>,
    skipped_busy: u64,
    overwritten: u64,
}
#[derive(Resource, Default, Clone)]
pub struct Mailbox(Arc<Mutex<Results>>);
impl Mailbox {
    fn push(&self, sample: Observation) {
        let mut results = self.0.lock().unwrap();
        if results.pending.len() == 4 {
            results.pending.remove(0);
            results.overwritten += 1;
        }
        results.pending.push(sample);
    }
    pub fn take(&self) -> (Vec<Observation>, u64, u64) {
        let mut results = self.0.lock().unwrap();
        (
            std::mem::take(&mut results.pending),
            results.skipped_busy,
            results.overwritten,
        )
    }
}
struct Slot {
    buffer: Buffer,
    busy: Arc<AtomicBool>,
}
#[derive(Resource, Default)]
struct Slots(Vec<Slot>);

pub fn install(app: &mut App, settings: AestraParticleLightSettings, allocations: bool) {
    install_frame_timing(app);
    let mailbox = Mailbox::default();
    let clusters = clusters::Mailbox::default();
    app.insert_resource(settings)
        .insert_resource(Start {
            ready: false,
            waiting_since: std::time::Instant::now(),
        })
        .init_resource::<crate::CaptureRenderReadiness>()
        // Measurement uses exact forward playback, not editor catch-up pacing.
        .insert_resource(aestra_bevy::gpu::AestraCatchupPacing { paced: false })
        .insert_resource(mailbox.clone())
        .insert_resource(clusters.clone())
        .init_resource::<Tick>()
        .add_plugins(ExtractResourcePlugin::<Tick>::default())
        .add_systems(Update, start.before(aestra_bevy::AestraSet::Playback));
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .insert_resource(clusters::allocations::Settings(allocations))
            .insert_resource(mailbox)
            .insert_resource(clusters)
            .init_resource::<Slots>()
            .add_systems(ExtractSchedule, crate::publish_capture_render_readiness)
            .add_systems(
                Render,
                (copy_counts, clusters::observe).in_set(RenderSystems::Cleanup),
            );
    }
}
/// Render-graph timestamp scope without selected-light counter readback or playback control.
pub(crate) fn install_frame_timing(app: &mut App) {
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(
            RenderGraph,
            (
                begin_frame
                    .in_set(RenderGraphSystems::Begin)
                    .after(begin_diagnostics_frame),
                end_frame
                    .after(RenderGraphSystems::Render)
                    .after(
                        aestra_bevy::gpu::particle_light_readback::ParticleLightReadbackSet::Copy,
                    )
                    .before(resolve_encoder)
                    .before(RenderGraphSystems::Submit),
            ),
        );
    }
}
// Bevy diagnostic spans are thread-local. Exclusive systems run on the schedule
// thread, so a span spanning several render systems must start and end here,
// not on independently scheduled workers.
fn begin_frame(world: &mut World) {
    let mut state = SystemState::<RenderContext>::new(world);
    let mut context = state.get_mut(world).expect("render context is initialized");
    let recorder = context.diagnostic_recorder();
    recorder.as_deref().begin_time_span(
        context.command_encoder(),
        "aestra::bench::full_frame".into(),
    );
    state.apply(world);
}
fn end_frame(world: &mut World) {
    let mut state = SystemState::<RenderContext>::new(world);
    let mut context = state.get_mut(world).expect("render context is initialized");
    let recorder = context.diagnostic_recorder();
    recorder.as_deref().end_time_span(context.command_encoder());
    state.apply(world);
}

fn copy_counts(
    state: Res<GpuSelectedParticleLights>,
    tick: Res<Tick>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mailbox: Res<Mailbox>,
    mut slots: ResMut<Slots>,
) {
    let mut observation = Observation {
        tick: *tick,
        sequence: None,
        source_runs: 0,
        selected_capacity: 0,
        scratch_bytes: None,
        reserved_bytes: None,
        rejected_outputs: 0,
        counters: None,
        status: state
            .rejection
            .clone()
            .unwrap_or_else(|| "empty_or_unprepared".into()),
    };
    let Some(frame) = state.frame() else {
        mailbox.push(observation);
        return;
    };
    observation.sequence = Some(frame.sequence);
    observation.source_runs = frame.manifest.len();
    observation.selected_capacity = frame.selected_capacity;
    observation.scratch_bytes = Some(frame.scratch_bytes);
    observation.reserved_bytes = Some(frame.reserved_bytes);
    observation.rejected_outputs = frame.rejected_outputs;
    let slot = slots
        .0
        .iter()
        .position(|s| !s.busy.load(Ordering::Acquire))
        .or_else(|| {
            if slots.0.len() == 3 {
                return None;
            }
            slots.0.push(Slot {
                buffer: device.create_buffer(&BufferDescriptor {
                    label: Some("particle-light benchmark counters"),
                    size: 16,
                    usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                busy: Arc::new(AtomicBool::new(false)),
            });
            Some(slots.0.len() - 1)
        });
    let Some(index) = slot else {
        mailbox.0.lock().unwrap().skipped_busy += 1;
        return;
    };
    let slot = &slots.0[index];
    slot.busy.store(true, Ordering::Release);
    let buffer = slot.buffer.clone();
    let busy = slot.busy.clone();
    let mailbox = mailbox.clone();
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&frame.counters, 0, &buffer, 0, 16);
    encoder.map_buffer_on_submit(&buffer.clone(), MapMode::Read, 0..16, move |result| {
        if result.is_ok() {
            let bytes = buffer.slice(..).get_mapped_range();
            let words = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| u32::from_le_bytes(*v))
                .collect::<Vec<_>>();
            observation.counters = Some(words.try_into().unwrap());
            observation.status = "selected".into();
            drop(bytes);
            buffer.unmap();
        } else {
            observation.status = "map_failed".into();
        }
        mailbox.push(observation);
        busy.store(false, Ordering::Release);
    });
    queue.submit([encoder.finish()]);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cluster_preallocation_is_tiered_and_shared_with_disabled_adapter_controls() {
        assert_eq!(cluster_capacities("high"), [4096, 524288]);
        assert_eq!(cluster_capacities("medium"), [2048, 262144]);
        assert_eq!(cluster_capacities("low"), [1024, 131072]);
    }
    #[test]
    fn native_adaptive_target_matches_preallocation_without_changing_cpu_fallback() {
        use bevy::light::cluster::{GlobalClusterGpuSettings, GlobalClusterSettings};
        let mut settings = GlobalClusterSettings {
            supports_storage_buffers: true,
            clustered_decals_are_usable: false,
            gpu_clustering: Some(GlobalClusterGpuSettings {
                initial_z_slice_list_capacity: 1,
                initial_index_list_capacity: 1,
            }),
            max_uniform_buffer_clusterable_objects: 204,
            view_cluster_bindings_max_indices: 16_384,
        };
        for tier in ["high", "medium", "low"] {
            configure_clusters(tier, &mut settings);
            let expected = cluster_capacities(tier);
            let gpu = settings.gpu_clustering.unwrap();
            assert_eq!(
                [
                    gpu.initial_z_slice_list_capacity,
                    gpu.initial_index_list_capacity
                ],
                expected
            );
            assert_eq!(settings.view_cluster_bindings_max_indices, expected[1]);
        }
        settings.gpu_clustering = None;
        settings.view_cluster_bindings_max_indices = 16_384;
        configure_clusters("high", &mut settings);
        assert_eq!(settings.view_cluster_bindings_max_indices, 16_384);
    }
    #[test]
    fn forward_benchmark_waits_for_shader_startup_without_a_seek() {
        let mut app = App::new();
        app.insert_resource(Start {
            ready: false,
            waiting_since: std::time::Instant::now(),
        })
        .init_resource::<crate::CaptureRenderReadiness>()
        .add_message::<AppExit>()
        .add_systems(Update, start);
        let mut player = aestra_bevy::EffectPlayer::new(&EffectAsset::new("startup", 12.0));
        player.playing = false;
        let entity = app.world_mut().spawn(player).id();
        app.update();
        assert!(!app.world().resource::<Start>().ready);
        assert!(
            !app.world()
                .get::<aestra_bevy::EffectPlayer>(entity)
                .unwrap()
                .playing
        );
        app.world_mut()
            .resource_mut::<crate::CaptureRenderReadiness>()
            .ready = true;
        app.update();
        assert!(app.world().resource::<Start>().ready);
        assert!(
            app.world()
                .get::<aestra_bevy::EffectPlayer>(entity)
                .unwrap()
                .playing
        );
    }

    #[test]
    fn async_mailbox_is_bounded_and_preserves_origin_tags() {
        let mailbox = Mailbox::default();
        for index in 0..7 {
            mailbox.push(Observation {
                tick: Tick {
                    index,
                    measured: index >= 5,
                    host_elapsed_seconds: index as f32,
                },
                sequence: Some(index),
                source_runs: 1,
                selected_capacity: 2,
                scratch_bytes: Some(128),
                reserved_bytes: Some(256),
                rejected_outputs: 0,
                counters: Some([7, 5, 2, 3]),
                status: "selected".into(),
            });
        }
        let (samples, skipped, overwritten) = mailbox.take();
        assert_eq!((samples.len(), skipped, overwritten), (4, 0, 3));
        assert_eq!(
            samples.iter().map(|s| s.tick.index).collect::<Vec<_>>(),
            [3, 4, 5, 6]
        );
        assert_eq!(samples.iter().filter(|s| s.tick.measured).count(), 2);
        assert!(mailbox.take().0.is_empty());
    }

    #[test]
    fn fixtures_keep_normal_draws_events_and_shared_show_sources() {
        let source = crate::fireworks_hero::effect();
        let mut fixture = source.clone();
        add_outputs(&mut fixture);
        let mut repeated = source.clone();
        add_outputs(&mut repeated);
        assert_eq!(fixture, repeated);
        assert_eq!(source.events, fixture.events);
        assert_eq!(source.material_instances, fixture.material_instances);
        assert_eq!(source.effect_clips, fixture.effect_clips);
        assert!(fixture.emitters.iter().any(|e| !e.scene_outputs.is_empty()));
        for output in fixture.emitters.iter().flat_map(|e| &e.scene_outputs) {
            let aestra_bevy::SceneOutputProperties::ParticlePointLight(light) = &output.properties;
            assert_eq!(light.max_lights_by_quality["high"], 32);
            assert_eq!(light.max_lights_by_quality["medium"], 16);
            assert_eq!(light.max_lights_by_quality["low"], 24);
            assert_eq!(light.range_curve.sample(0.0), 12.0);
            assert_eq!(light.intensity_curve.sample(0.0), 1500.0);
        }
        for (before, after) in source.emitters.iter().zip(&fixture.emitters) {
            let mut restored = after.clone();
            restored.scene_outputs = before.scene_outputs.clone();
            assert_eq!(*before, restored);
        }
        let index = aestra_project::ProjectAssetIndex::scan(crate::viewer_asset_root(None));
        let mut project = index
            .resolve_effect_project(&crate::fireworks_show::effect())
            .unwrap();
        for effect in std::iter::once(&mut project.root).chain(project.dependencies.values_mut()) {
            add_outputs(effect);
        }
        for tier in aestra_bevy::QualityTier::presets() {
            let compiled = aestra_bevy::EffectCompiler::default()
                .with_tier(tier)
                .compile_resolved_project(&project)
                .unwrap();
            assert_eq!(compiled.dependencies.len(), 4);
            assert!(
                compiled
                    .dependencies
                    .values()
                    .all(|e| e.emitters.iter().any(|e| !e.scene_outputs.is_empty()))
            );
        }
    }
}
