//! Native dependency regression. No selected-particle readback or runtime changes.
use bevy::{
    camera::{RenderTarget, ShadowLodOrigin},
    light::cluster::{
        ClusterConfig, ClusterFarZMode, ClusterZConfig, Clusters, GlobalClusterSettings,
    },
    pbr::ViewClusterBindings,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{BindingResource, TextureFormat},
        sync_world::MainEntity,
    },
};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
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
