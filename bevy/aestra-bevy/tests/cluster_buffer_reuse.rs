//! Native dependency regression. No selected-particle readback or runtime changes.
use bevy::{
    camera::{RenderTarget, ShadowLodOrigin},
    light::cluster::{
        ClusterConfig, ClusterFarZMode, ClusterZConfig, Clusters, GlobalClusterSettings,
    },
    pbr::{AestraClusterReadbackProbe, ViewClusterBindings, aestra_cluster_diagnostics},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{BindingResource, BufferId, TextureFormat},
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
    },
};
use std::{
    collections::{BTreeMap, HashSet},
    hash::{Hash, Hasher},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    identity: u64,
    bytes: u64,
}
fn binding(resource: Option<BindingResource<'_>>) -> Binding {
    let Some(BindingResource::Buffer(binding)) = resource else {
        panic!("Missing native buffer")
    };
    let mut identity = std::collections::hash_map::DefaultHasher::new();
    binding.buffer.hash(&mut identity);
    Binding {
        identity: identity.finish(),
        bytes: binding.buffer.size(),
    }
}
type ObservedViews = BTreeMap<Entity, (Binding, Binding)>;
#[derive(Resource, Clone, Default)]
struct Observations(Arc<Mutex<Vec<ObservedViews>>>);
fn observe(views: Query<(&MainEntity, &ViewClusterBindings)>, results: Res<Observations>) {
    results.0.lock().unwrap().push(
        views
            .iter()
            .map(|(entity, bindings)| {
                (
                    entity.id(),
                    (
                        binding(bindings.clusterable_object_index_lists_binding()),
                        binding(bindings.offsets_and_counts_binding()),
                    ),
                )
            })
            .collect(),
    );
}
fn grid(x: u32, y: u32, z: u32) -> ClusterConfig {
    ClusterConfig::XYZ {
        dimensions: UVec3::new(x, y, z),
        z_config: ClusterZConfig {
            first_slice_depth: 0.1,
            far_z_mode: ClusterFarZMode::Constant(30.0),
        },
        dynamic_resizing: false,
    }
}
fn camera(app: &mut App, order: isize) -> Entity {
    let image = Image::new_target_texture(320, 180, TextureFormat::Bgra8UnormSrgb, None);
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.world_mut()
        .spawn((
            Camera3d::default(),
            Camera { order, ..default() },
            RenderTarget::Image(target.into()),
            ShadowLodOrigin,
            grid(2, 2, 4),
            Transform::from_xyz(0.0, 3.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id()
}
fn window(app: &mut App, observations: &Observations) -> ObservedViews {
    // Fixed observation windows after settling, not a timing claim.
    for _ in 0..24 {
        app.update();
        std::thread::sleep(Duration::from_millis(4));
    }
    observations.0.lock().unwrap().clear();
    for _ in 0..12 {
        app.update();
        std::thread::sleep(Duration::from_millis(4));
    }
    let results = observations.0.lock().unwrap();
    assert_eq!(results.len(), 12);
    let expected = results.last().unwrap();
    assert!(
        results.iter().all(|r| r == expected),
        "Native bindings churned in a settled window"
    );
    expected.clone()
}
fn demand(app: &App, view: Entity) -> usize {
    app.world()
        .get::<Clusters>(view)
        .unwrap()
        .last_frame_total_cluster_index_count
        .expect("Unavailable native asynchronous index demand")
}

#[test]
#[ignore = "native GPU buffer lifetime/growth/mode/view regression; run explicitly, alone"]
fn native_cluster_buffers_reuse_grow_reset_and_retire_per_view() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    );
    app.finish();
    app.cleanup();
    let gpu_settings = app
        .world()
        .resource::<GlobalClusterSettings>()
        .gpu_clustering
        .expect("Native GPU clustering required");
    let observations = Observations::default();
    app.sub_app_mut(RenderApp)
        .insert_resource(observations.clone())
        .add_systems(Render, observe.after(RenderSystems::PrepareBindGroups));
    let first = camera(&mut app, 0);
    let deadline = Instant::now() + Duration::from_secs(45);
    while app
        .world()
        .get::<Clusters>(first)
        .and_then(|c| c.last_frame_total_cluster_index_count)
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "Native pipelines/statistics did not become ready"
        );
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    let baseline = window(&mut app, &observations);
    let empty_demand = demand(&app, first);
    assert_eq!(baseline.len(), 1);
    // Native empty storage inserts a default clustered light; compare retirement
    // with its measured baseline, not an invented zero-index expectation.
    let light = app
        .world_mut()
        .spawn((
            PointLight {
                intensity: 1000.0,
                range: 100.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(0.0, 2.0, 0.0),
        ))
        .id();
    let initial = window(&mut app, &observations);
    assert_eq!(initial.len(), 1);
    let original_demand = demand(&app, first);
    assert!(
        original_demand > empty_demand,
        "Fixture must distinguish the active light from native empty storage"
    );
    assert_eq!(initial, baseline);
    // Logical counts and metadata must reset; repeated steady frames do not accumulate.
    assert_eq!(window(&mut app, &observations), initial);
    assert_eq!(demand(&app, first), original_demand);
    app.world_mut().entity_mut(first).insert(grid(4, 4, 8));
    let grown = window(&mut app, &observations);
    assert_eq!(
        grown[&first].0, initial[&first].0,
        "Index capacity did not need growth"
    );
    assert!(grown[&first].1.bytes > initial[&first].1.bytes);
    assert_ne!(grown[&first].1.identity, initial[&first].1.identity);
    app.world_mut().entity_mut(first).insert(grid(2, 2, 4));
    assert_eq!(
        window(&mut app, &observations),
        grown,
        "Shrinking should retain physical high-water capacity"
    );
    assert_eq!(demand(&app, first), original_demand);
    // A second view has independent owning containers, not a global buffer cache.
    let second = camera(&mut app, 1);
    let two = window(&mut app, &observations);
    assert_eq!(two.len(), 2);
    assert_eq!(two[&first], grown[&first]);
    assert_ne!(two[&first].0.identity, two[&second].0.identity);
    assert_ne!(two[&first].1.identity, two[&second].1.identity);
    app.world_mut().get_mut::<Camera>(second).unwrap().is_active = false;
    let hidden = window(&mut app, &observations);
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[&first], grown[&first]);
    app.world_mut().get_mut::<Camera>(second).unwrap().is_active = true;
    let reactivated = window(&mut app, &observations);
    assert_eq!(reactivated.len(), 2);
    assert_eq!(reactivated[&first], grown[&first]);
    assert_ne!(reactivated[&second].0.identity, two[&second].0.identity);
    app.world_mut().despawn(second);
    assert_eq!(window(&mut app, &observations), grown);
    // Existing CPU fallback remains usable; resume GPU preparation after a mode switch.
    app.world_mut()
        .resource_mut::<GlobalClusterSettings>()
        .gpu_clustering = None;
    for _ in 0..12 {
        app.update();
    }
    app.world_mut()
        .resource_mut::<GlobalClusterSettings>()
        .gpu_clustering = Some(gpu_settings);
    let resumed = window(&mut app, &observations);
    assert_eq!(resumed.len(), 1);
    assert_eq!(demand(&app, first), original_demand);
    app.world_mut().despawn(light);
    let empty = window(&mut app, &observations);
    assert_eq!(empty, resumed);
    assert_eq!(
        demand(&app, first),
        empty_demand,
        "Old lights/metadata survived retirement"
    );
    app.world_mut().despawn(first);
    assert!(window(&mut app, &observations).is_empty());
    println!(
        "cluster_reuse accepted=true settled_windows=12 updates_per_window=12 settling_updates=24 empty_demand={empty_demand} active_demand={original_demand} index_bytes={} small_offsets_bytes={} grown_offsets_bytes={}",
        initial[&first].0.bytes, initial[&first].1.bytes, grown[&first].1.bytes
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PrivateGeneration {
    buffers: [(BufferId, u64); 3],
    logical_scratchpad_length: usize,
    z_capacity: usize,
    index_capacity: usize,
}
type PrivateViews = BTreeMap<Entity, PrivateGeneration>;
#[derive(Resource, Clone, Default)]
struct PrivateObservations(Arc<Mutex<Vec<PrivateViews>>>, Arc<AtomicUsize>);

fn observe_private(world: &mut World) {
    let snapshots = aestra_cluster_diagnostics(world);
    let mut views = PrivateViews::new();
    for snapshot in snapshots {
        let readback = snapshot.readback.as_ref().unwrap().snapshot().unwrap();
        let pool_count = readback.pending.len() + readback.free.len();
        assert!(
            pool_count <= 8,
            "Unbounded native staging references: {pool_count}"
        );
        let unique: HashSet<_> = readback.pending.iter().chain(&readback.free).collect();
        assert_eq!(
            unique.len(),
            pool_count,
            "Duplicate native staging generation"
        );
        world
            .resource::<PrivateObservations>()
            .1
            .fetch_max(pool_count, Ordering::Relaxed);
        views.insert(
            snapshot.view,
            PrivateGeneration {
                buffers: snapshot
                    .private_buffers
                    .map(|buffer| buffer.expect("Missing private buffer")),
                logical_scratchpad_length: snapshot.scratchpad_logical_length,
                z_capacity: readback.z_slice_capacity,
                index_capacity: readback.index_capacity,
            },
        );
    }
    world
        .resource::<PrivateObservations>()
        .0
        .lock()
        .unwrap()
        .push(views);
}

fn private_window(
    app: &mut App,
    observations: &PrivateObservations,
    updates: usize,
) -> PrivateViews {
    for _ in 0..24 {
        app.update();
        std::thread::sleep(Duration::from_millis(4));
    }
    observations.0.lock().unwrap().clear();
    for _ in 0..updates {
        app.update();
        std::thread::sleep(Duration::from_millis(4));
    }
    let results = observations.0.lock().unwrap();
    assert_eq!(results.len(), updates);
    let last = results.last().unwrap();
    assert!(
        results.iter().all(|r| r == last),
        "Private generations/capacities churned after settling"
    );
    for (view, generation) in last {
        println!(
            "cluster_generation view={view:?} steady_updates={updates} buffers={:?} scratchpad_length={} z_capacity={} index_capacity={}",
            generation.buffers,
            generation.logical_scratchpad_length,
            generation.z_capacity,
            generation.index_capacity
        );
    }
    last.clone()
}

fn probe(app: &mut App, view: Entity) -> AestraClusterReadbackProbe {
    aestra_cluster_diagnostics(app.sub_app_mut(RenderApp).world_mut())
        .into_iter()
        .find(|s| s.view == view)
        .unwrap()
        .readback
        .unwrap()
}

fn completed_checkpoint(app: &mut App, phase: &str, expected_views: usize) {
    // Test-only asynchronous submitted-work boundary; never PollType::Wait.
    // This proves named allocator retirement after prior submissions complete,
    // not the exact driver free instant or total process VRAM.
    let completed = Arc::new(AtomicBool::new(false));
    let callback = completed.clone();
    app.sub_app(RenderApp)
        .world()
        .resource::<RenderQueue>()
        .on_submitted_work_done(move || callback.store(true, Ordering::Release));
    let deadline = Instant::now() + Duration::from_secs(45);
    while !completed.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline, "Submitted work did not complete");
        app.update();
        std::thread::sleep(Duration::from_millis(4));
    }
    let report = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .generate_allocator_report()
        .expect("Native allocator report required");
    for name in [
        "clustering Z slice buffer",
        "clustering scratchpad buffer",
        "clustering Z slicing metadata buffer",
    ] {
        let allocations: Vec<_> = report
            .allocations
            .iter()
            .filter(|a| a.name == name)
            .collect();
        assert_eq!(
            allocations.len(),
            expected_views,
            "Unexpected settled named allocation count for {name}"
        );
        let bytes: u64 = allocations.iter().map(|a| a.size).sum();
        println!(
            "cluster_private phase={phase} name={name:?} live_allocations={} bytes={bytes}",
            allocations.len()
        );
    }
    let staging: Vec<_> = report
        .allocations
        .iter()
        .filter(|a| a.name == "clustering metadata staging buffer")
        .collect();
    assert!(
        staging.len() <= expected_views * 8,
        "Staging allocation ceiling exceeded"
    );
    println!(
        "cluster_private phase={phase} staging_allocations={} allocated_bytes={} reserved_bytes={} submitted_work_completed=true",
        staging.len(),
        report.total_allocated_bytes,
        report.total_reserved_bytes
    );
}

#[test]
#[ignore = "native private generations/overflow/retirement regression; run explicitly, alone"]
fn native_private_generations_overflow_recover_and_retire() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    );
    app.finish();
    app.cleanup();
    let settings = app
        .world_mut()
        .resource_mut::<GlobalClusterSettings>()
        .into_inner();
    let gpu = settings
        .gpu_clustering
        .as_mut()
        .expect("Native GPU clustering required");
    // Intentionally undersized feedback capacities, not production settings.
    gpu.initial_z_slice_list_capacity = 32;
    gpu.initial_index_list_capacity = 64;
    let observations = PrivateObservations::default();
    app.sub_app_mut(RenderApp)
        .insert_resource(observations.clone())
        .add_systems(
            Render,
            observe_private.after(RenderSystems::PrepareBindGroups),
        );
    let first = camera(&mut app, 0);
    app.world_mut().entity_mut(first).insert(grid(4, 4, 8));
    let deadline = Instant::now() + Duration::from_secs(45);
    while app
        .world()
        .get::<Clusters>(first)
        .and_then(|c| c.last_frame_total_cluster_index_count)
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "Native pipelines/statistics not ready"
        );
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    let baseline = private_window(&mut app, &observations, 12);
    let empty = demand(&app, first);
    assert_eq!(baseline[&first].z_capacity, 32);
    assert_eq!(baseline[&first].index_capacity, 64);
    assert_eq!(baseline[&first].logical_scratchpad_length, 128);
    completed_checkpoint(&mut app, "baseline", 1);
    let lights: Vec<_> = (0..64)
        .map(|_| {
            app.world_mut()
                .spawn((
                    PointLight {
                        intensity: 1000.0,
                        range: 100.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 2.0, 0.0),
                ))
                .id()
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(45);
    while demand(&app, first) != 8192 {
        assert!(
            Instant::now() < deadline,
            "Overflow did not recover to 64 lights x 128 clusters"
        );
        app.update();
        std::thread::sleep(Duration::from_millis(4));
    }
    let recovered = private_window(&mut app, &observations, 240);
    assert_eq!(demand(&app, first), 8192);
    assert_eq!(recovered[&first].z_capacity, 512);
    assert_eq!(recovered[&first].index_capacity, 8192);
    assert_eq!(recovered[&first].buffers[0].1, 6144);
    assert_ne!(
        recovered[&first].buffers[0].0,
        baseline[&first].buffers[0].0
    );
    assert_eq!(
        recovered[&first].buffers[1..],
        baseline[&first].buffers[1..]
    );
    assert_eq!(recovered[&first].logical_scratchpad_length, 128);
    println!(
        "cluster_overflow recovered=true empty_demand={empty} active_demand=8192 z_capacity=512 index_capacity=8192 steady_updates=240"
    );
    completed_checkpoint(&mut app, "overflow-recovered", 1);
    for light in lights {
        app.world_mut().despawn(light);
    }
    assert_eq!(private_window(&mut app, &observations, 12), recovered);
    assert_eq!(demand(&app, first), empty);
    completed_checkpoint(&mut app, "lights-removed", 1);
    let second = camera(&mut app, 1);
    let two = private_window(&mut app, &observations, 12);
    assert_eq!(two.len(), 2);
    assert_eq!(two[&first], recovered[&first]);
    for i in 0..3 {
        assert_ne!(two[&first].buffers[i].0, two[&second].buffers[i].0);
    }
    completed_checkpoint(&mut app, "two-views", 2);
    let retired_second = probe(&mut app, second);
    app.world_mut().get_mut::<Camera>(second).unwrap().is_active = false;
    assert_eq!(private_window(&mut app, &observations, 12), recovered);
    completed_checkpoint(&mut app, "second-inactive", 1);
    assert!(
        retired_second.snapshot().is_none(),
        "Callback state kept inactive view alive"
    );
    app.world_mut().get_mut::<Camera>(second).unwrap().is_active = true;
    let reactivated = private_window(&mut app, &observations, 12);
    for i in 0..3 {
        assert_ne!(reactivated[&second].buffers[i].0, two[&second].buffers[i].0);
    }
    let retired_second = probe(&mut app, second);
    app.world_mut().despawn(second);
    assert_eq!(private_window(&mut app, &observations, 12), recovered);
    completed_checkpoint(&mut app, "second-despawned", 1);
    assert!(retired_second.snapshot().is_none());
    let retired_gpu_mode = probe(&mut app, first);
    let settings = app
        .world()
        .resource::<GlobalClusterSettings>()
        .gpu_clustering
        .unwrap();
    app.world_mut()
        .resource_mut::<GlobalClusterSettings>()
        .gpu_clustering = None;
    assert!(private_window(&mut app, &observations, 12).is_empty());
    completed_checkpoint(&mut app, "cpu-mode", 0);
    assert!(
        retired_gpu_mode.snapshot().is_none(),
        "CPU mode retained native readback state"
    );
    app.world_mut()
        .resource_mut::<GlobalClusterSettings>()
        .gpu_clustering = Some(settings);
    let resumed = private_window(&mut app, &observations, 12);
    assert_eq!(resumed.len(), 1);
    assert_eq!(demand(&app, first), empty);
    for i in 0..3 {
        assert_ne!(resumed[&first].buffers[i].0, recovered[&first].buffers[i].0);
    }
    completed_checkpoint(&mut app, "gpu-resumed", 1);
    let retired_first = probe(&mut app, first);
    app.world_mut().despawn(first);
    assert!(private_window(&mut app, &observations, 12).is_empty());
    completed_checkpoint(&mut app, "all-views-removed", 0);
    assert!(
        retired_first.snapshot().is_none(),
        "Callback state survived completed submissions"
    );
    println!(
        "cluster_private accepted=true windows=10 private_generations=stable pool_reference_ceiling=8 retired_weak_owners=4 final_named_allocations=0"
    );
    println!(
        "cluster_private observed_staging_pool_peak_per_view={}",
        observations.1.load(Ordering::Relaxed)
    );
}
