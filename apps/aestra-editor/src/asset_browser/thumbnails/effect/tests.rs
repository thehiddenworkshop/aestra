use super::*;
use aestra_core::{EffectAsset, Emitter};

fn project() -> ResolvedEffectProject {
    let mut root = EffectAsset::new("Thumbnail", 3.0);
    root.emitters.push(Emitter::basic_sprite("Particles", 3.0));
    ResolvedEffectProject {
        root,
        dependencies: BTreeMap::new(),
        material_programs: BTreeMap::new(),
        material_functions: BTreeMap::new(),
    }
}

#[test]
fn sample_time_is_deterministic_and_bounded() {
    assert_eq!(sample_time(3.0).unwrap(), 1.5);
    assert_eq!(sample_time(10.0).unwrap(), 2.0);
    assert!(sample_time(f32::NAN).is_err());
    assert!(sample_time(0.0).is_err());
}

#[test]
fn saved_players_and_framing_are_deterministic_and_do_not_modify_source() {
    let saved = project();
    let original = saved.root.clone();
    let flag = AtomicBool::new(false);
    let a = prepare(saved.clone(), Path::new("."), &flag).unwrap();
    let b = prepare(saved.clone(), Path::new("."), &flag).unwrap();
    assert_eq!(saved.root, original);
    assert_eq!(a.center, b.center);
    assert_eq!(a.radius, b.radius);
    assert!(a.radius > 0.0 && a.radius.is_finite());
    assert_eq!(a.players.len(), 1);
    assert_eq!(a.players[0].simulation_time(), 1.5);
    assert_eq!(a.players[0].instance.seed(), SEED);
}

#[test]
fn cancelled_and_over_budget_effects_fail_before_gpu_work() {
    assert_eq!(
        prepare(project(), Path::new("."), &AtomicBool::new(true))
            .err()
            .unwrap(),
        "Cancelled"
    );
    let mut saved = project();
    saved.root.emitters[0].max_particles = 4097;
    assert!(
        prepare(saved, Path::new("."), &AtomicBool::new(false))
            .err()
            .unwrap()
            .contains("limit")
    );
}

#[test]
fn empty_effect_uses_an_explained_fallback() {
    let mut saved = project();
    saved.root.emitters.clear();
    assert!(
        prepare(saved, Path::new("."), &AtomicBool::new(false))
            .err()
            .unwrap()
            .contains("No particles")
    );
}

fn trail_project(root: &Path) -> ResolvedEffectProject {
    fixture_project(root, "trail_lab")
}

fn fixture_project(root: &Path, name: &str) -> ResolvedEffectProject {
    let content = ProjectContent::scan(root);
    let asset = EffectAsset::load_ron(root.join(format!("effects/{name}.aestra.ron"))).unwrap();
    content
        .cached_effect_project_with_materials(&asset, BTreeMap::new())
        .unwrap()
}

#[test]
fn mesh_effect_preserves_geometry_attributes_and_bounds_displacement() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let saved = fixture_project(&root, "mesh_material_lab");
    let prepared = prepare(saved.clone(), &root, &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.meshes.len(), 1);
    let mesh = &prepared.meshes[0].1;
    for attribute in [
        Mesh::ATTRIBUTE_NORMAL,
        Mesh::ATTRIBUTE_UV_0,
        Mesh::ATTRIBUTE_UV_1,
        Mesh::ATTRIBUTE_TANGENT,
    ] {
        assert!(mesh.contains_attribute(attribute));
    }
    assert!(prepared.radius.is_finite() && prepared.radius > 0.0);
    let effect = prepared.players[0].effect();
    let program = effect
        .material_programs
        .iter()
        .find(|p| p.outputs.vertex_offset.is_some())
        .unwrap();
    let instance = effect
        .material_instances
        .iter()
        .find(|i| i.program.id() == program.id)
        .unwrap();
    assert!(displacement::radius(program, instance, 1.0).unwrap() > 1.0);
    let mut unsupported = program.clone();
    let output = unsupported.outputs.vertex_offset.unwrap();
    unsupported
        .expressions
        .iter_mut()
        .find(|e| e.id == output)
        .unwrap()
        .kind = MaterialExpressionKind::Input(aestra_core::material::MaterialInput::WorldPosition);
    assert!(displacement::radius(&unsupported, instance, 1.0).is_err());
    let again = prepare(saved, &root, &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.radius, again.radius);
    for reference in [
        "meshes/lab_cube.gltf#Mesh99/Primitive0",
        "../outside.gltf#Mesh0/Primitive0",
        "meshes/lab_cube.gltf#Scene0",
    ] {
        assert!(mesh::load_primitive(&root, reference, &AtomicBool::new(false)).is_err());
    }
}

#[test]
fn trail_fixture_has_bounded_history_and_framing() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let prepared = prepare(trail_project(&root), &root, &AtomicBool::new(false)).unwrap();
    assert!(prepared.radius > 0.0);
    assert!(
        prepared
            .players
            .iter()
            .any(|p| p.effect().emitters.iter().any(|e| e
                .renderers
                .iter()
                .any(|r| matches!(r.kind, RendererPlanKind::Trail { .. }))))
    );
}

#[test]
fn texture_pixels_are_fresh_job_owned_and_paths_stay_in_the_project() {
    let root = tempfile::tempdir().unwrap();
    let mut saved = project();
    saved
        .root
        .assets
        .push(aestra_core::AssetDefinition::texture(
            "Sample",
            "sample.png",
        ));
    let file = root.path().join("sample.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
        .save(&file)
        .unwrap();
    let first = prepare(saved.clone(), root.path(), &AtomicBool::new(false)).unwrap();
    image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 255, 0, 255]))
        .save(&file)
        .unwrap();
    let second = prepare(saved.clone(), root.path(), &AtomicBool::new(false)).unwrap();
    assert_ne!(first.textures[0].1.data, second.textures[0].1.data);
    saved.root.assets[0].path = "../outside.png".into();
    // The compiler may reject the registry path before the bounded file reader sees it.
    assert!(prepare(saved, root.path(), &AtomicBool::new(false)).is_err());
}

#[test]
fn effects_without_a_renderer_show_fallback_without_touching_the_active_document() {
    let root = tempfile::tempdir().unwrap();
    let saved = project();
    saved
        .root
        .save_ron(root.path().join("effect.aestra.ron"))
        .unwrap();
    let mut app = crate::asset_browser::tests::browser_app(root.path());
    let before = app.world().resource::<EditorSession>().effect.clone();
    super::super::tests::finish_jobs(&mut app);
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    let cache = app.world().resource::<ThumbnailCache>();
    assert!(
        cache
            .entries
            .values()
            .any(|e| matches!(&e.preview, Preview::Failed(s) if s.contains("GPU")))
    );
}

// Run explicitly on a native GPU machine. It uses no window, user project edits, or active
// editor state, and exercises the actual render/capture/cleanup path rather than mock pixels.
#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_gpu_trail_thumbnail_captures_pixels_and_cleans_up() {
    capture_fixture("trail_lab", true);
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_gpu_mesh_thumbnail_captures_pixels_and_cleans_up() {
    capture_fixture("mesh_material_lab", true);
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_gpu_prism_thumbnail_fills_the_tile() {
    capture_fixture("prism_bloom", true);
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_gpu_project_mesh_uses_project_root_without_thumbnail_overrides() {
    capture_fixture("mesh_material_lab", false);
}

fn capture_fixture(name: &str, private_assets: bool) {
    use bevy::{app::PluginsState, ecs::system::RunSystemOnce, window::ExitCondition};
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/test")
        .canonicalize()
        .unwrap();
    let mut prepared =
        prepare(fixture_project(&root, name), &root, &AtomicBool::new(false)).unwrap();
    if !private_assets {
        prepared.meshes.clear();
        prepared.textures.clear();
    }
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::asset::AssetPlugin {
                unapproved_path_mode: bevy::asset::UnapprovedPathMode::Deny,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy_winit::WinitPlugin>(),
    )
    .add_plugins(aestra_bevy_render::AestraRenderPlugin)
    .insert_resource(aestra_bevy_render::AestraTextureRoot(Some(root.clone())))
    .init_resource::<ThumbnailCache>();
    register(&mut app);
    let start = Instant::now();
    while app.plugins_state() != PluginsState::Ready {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "GPU initialization timed out"
        );
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(Duration::from_millis(10));
    }
    app.finish();
    app.cleanup();
    let epoch = (
        root,
        ProjectContentVersion {
            generation: 1,
            revision: 0,
        },
    );
    let mut prepared = Some(prepared);
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  mut images: ResMut<Assets<Image>>,
                  mut cache: ResMut<ThumbnailCache>,
                  mut context: Context| {
                cache.epoch = Some(epoch.clone());
                cache.gpu = Some(GpuJob::start(
                    prepared.take().unwrap(),
                    ProjectSourceId::from_u64(1),
                    epoch.clone(),
                    &mut commands,
                    &mut images,
                    &mut context,
                ));
            },
        )
        .unwrap();
    let (entities, target, textures, meshes) = {
        let job = app
            .world()
            .resource::<ThumbnailCache>()
            .gpu
            .as_ref()
            .unwrap();
        (
            job.spawned_entities(),
            job.target_id(),
            job.textures.iter().map(Handle::id).collect::<Vec<_>>(),
            job.meshes.iter().map(Handle::id).collect::<Vec<_>>(),
        )
    };
    loop {
        app.update();
        let result = app
            .world_mut()
            .run_system_once(
                |mut commands: Commands, mut cache: ResMut<ThumbnailCache>, context: Context| {
                    cache.gpu.as_mut().unwrap().poll(&mut commands, &context)
                },
            )
            .unwrap();
        if let Some(result) = result {
            let bytes = result.unwrap();
            assert_eq!(bytes.len(), (EDGE * EDGE * 4) as usize);
            let colors: BTreeSet<_> = bytes.as_chunks::<4>().0.iter().copied().collect();
            assert!(
                colors.len() > 16,
                "capture must contain visible effect shading, not just clear color"
            );
            // The result is rerendered with tighter camera framing, not a magnified tiny bitmap.
            let background = &bytes[..3];
            let mut min = UVec2::splat(EDGE);
            let mut max = UVec2::ZERO;
            for (i, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
                if (0..3).any(|c| pixel[c].abs_diff(background[c]) > 3) {
                    let p = UVec2::new(i as u32 % EDGE, i as u32 / EDGE);
                    min = min.min(p);
                    max = max.max(p);
                }
            }
            assert!(
                (max - min).max_element() >= 70,
                "effect should occupy most of the thumbnail: {min:?}..{max:?}"
            );
            let output =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/asset-thumbnail-smoke");
            fs::create_dir_all(&output).unwrap();
            image::RgbaImage::from_raw(EDGE, EDGE, bytes)
                .unwrap()
                .save(output.join(format!(
                    "{name}{}.png",
                    if private_assets { "" } else { "_project_root" }
                )))
                .unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    app.world_mut()
        .run_system_once(
            |mut commands: Commands,
             mut images: ResMut<Assets<Image>>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut cache: ResMut<ThumbnailCache>| {
                cache
                    .gpu
                    .take()
                    .unwrap()
                    .cleanup(&mut commands, &mut images, Some(&mut meshes));
            },
        )
        .unwrap();
    assert!(entities.iter().all(|e| app.world().get_entity(*e).is_err()));
    assert!(!app.world().resource::<Assets<Image>>().contains(target));
    assert!(
        meshes
            .iter()
            .all(|id| !app.world().resource::<Assets<Mesh>>().contains(*id))
    );
    assert!(
        textures
            .iter()
            .all(|id| !app.world().resource::<Assets<Image>>().contains(*id))
    );
}
