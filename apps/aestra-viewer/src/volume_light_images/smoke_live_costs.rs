//! F8.1C1: forward playback costs, not paused images or final-show art acceptance.
use super::*;
use aestra_bevy::gpu::{
    GpuAlphaSortStatistics, GpuEventLinkStatistics, GpuParticleStatistics, GpuSimulationTiming,
};
use serde_json::{Value as Json, json};

#[derive(Clone, Copy, Debug)]
pub(super) enum Case {
    Cohorts { roots: usize, draw: bool },
    Show,
    PersistentShow { draw: bool },
}

impl Case {
    fn label(self) -> String {
        match self {
            Self::Cohorts { roots, draw } => {
                format!("cohorts-{roots}-{}", if draw { "draw" } else { "no-draw" })
            }
            Self::Show => "show-unlit-baseline".into(),
            Self::PersistentShow { draw } => format!(
                "show-persistent-{}",
                if draw { "draw" } else { "no-smoke-draw" }
            ),
        }
    }

    fn frames(self) -> usize {
        match self {
            Self::Cohorts { .. } => 1080,
            Self::Show => 1680,
            Self::PersistentShow { .. } => 2400,
        }
    }
}

pub(super) fn fixture(tier: &str, case: Case) -> Arc<aestra_bevy::CompiledEffectProject> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let effect = EffectAsset::from_ron(
        &fs::read_to_string(root.join(match case {
            Case::Cohorts { .. } => "effects/fireworks_smoke_cohorts.aestra.ron",
            Case::Show => "effects/fireworks_show.aestra.ron",
            Case::PersistentShow { .. } => "effects/fireworks_show_persistent_smoke.aestra.ron",
        }))
        .unwrap(),
    )
    .unwrap();
    let resolved = aestra_project::ProjectAssetIndex::scan(root)
        .resolve_effect_project(&effect)
        .unwrap();
    let mut compiled = EffectCompiler::default()
        .with_tier(aestra_bevy::QualityTier::preset(tier).unwrap())
        .compile_resolved_project(&resolved)
        .unwrap();
    if let Case::Cohorts { draw: false, .. } = case {
        // Test-only presentation control, after normal authored validation/compilation.
        // A saved emitter without a renderer/output is intentionally invalid. Removing
        // only its compiled draw keeps the exact simulation/output artifact unchanged.
        Arc::make_mut(&mut compiled.root).emitters[0]
            .renderers
            .clear();
    }
    if let Case::PersistentShow { draw: false } = case {
        for child in compiled.dependencies.values_mut() {
            let child = Arc::make_mut(child);
            let smoke_materials: Vec<_> = child
                .material_instances
                .iter()
                .filter(|material| {
                    material.program.id().to_string() == "a3574a00-0000-4000-8000-000000f81200"
                })
                .map(|material| material.id)
                .collect();
            for emitter in &mut child.emitters {
                emitter
                    .renderers
                    .retain(|renderer| !smoke_materials.contains(&renderer.material));
            }
        }
    }
    Arc::new(compiled)
}

fn distribution(mut values: Vec<f64>) -> Json {
    if values.is_empty() {
        return Json::Null;
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    json!({"samples":n,"median_ms":values[n/2],"p95_ms":values[(n-1)*95/100],
        "p99_ms":values[(n-1)*99/100],"max_ms":values[n-1]})
}

fn run(tier: &str, case: Case) -> Json {
    let compiled = fixture(tier, case);
    let (mut app, owner, target) =
        headless_project_timing(tier, true, true, compiled.clone(), true);
    *app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&target)
        .unwrap() = Image::new_target_texture(960, 540, TextureFormat::Rgba8UnormSrgb, None);
    let mut owners = vec![owner];
    match case {
        Case::Cohorts { roots, .. } => {
            for _ in 1..roots {
                let mut player = EffectPlayer::from_project(compiled.clone())
                    .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
                player.set_seed(0xf83b_0000_0000_0001);
                player.playing = false;
                // Deliberately coincident roots: repeatable worst-case overlap, not
                // a saved choreography or a correctness claim for cross-draw sorting.
                owners.push(
                    app.world_mut()
                        .spawn((player, Transform::from_scale(Vec3::splat(0.1))))
                        .id(),
                );
            }
        }
        Case::Show | Case::PersistentShow { .. } => {
            *app.world_mut().get_mut::<Transform>(owner).unwrap() = Transform::IDENTITY;
            let mut cameras = app
                .world_mut()
                .query_filtered::<&mut Transform, With<Camera3d>>();
            for mut camera in cameras.iter_mut(app.world_mut()) {
                *camera = crate::fireworks_show::camera(crate::FireworksCamera::Audience);
            }
            // Match the public normal show's ordering and representative-only lights.
            app.world_mut()
                .resource_mut::<AestraSettings>()
                .transparent_order = if matches!(case, Case::Show) {
                aestra_bevy::TransparentOrderMode::Fast
            } else {
                aestra_bevy::TransparentOrderMode::DepthBackToFront
            };
            let mut policy = LightingQualityPolicy::preset(tier).unwrap();
            policy.particle.enabled = false;
            policy.particle.max_lights = 0;
            policy.apply(app.world_mut()).unwrap();
        }
    }
    // Await prepared shaders while paused, then warm the diagnostic transport. A
    // screenshot here is tooling only and is outside the live measurement window.
    let _ = capture(&mut app, &target);
    pump(&mut app, 120);
    let mut seen = BTreeMap::new();
    for diagnostic in app.world().resource::<DiagnosticsStore>().iter() {
        if let Some(value) = diagnostic.measurement() {
            seen.insert(diagnostic.path().as_str().to_owned(), value.time);
        }
    }
    for owner in &owners {
        app.world_mut()
            .get_mut::<EffectPlayer>(*owner)
            .unwrap()
            .playing = true;
    }
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(1.0 / 60.0),
    ));
    let mut metrics = BTreeMap::<String, Vec<f64>>::new();
    let mut simulation = BTreeMap::<String, Vec<f64>>::new();
    let mut sequences = BTreeMap::new();
    let (mut max_fixed_ticks, mut checkpoint_bytes) = (0, 0_u64);
    let mut wall = Vec::new();
    let (mut peak_alive, mut peak_smoke, mut peak_clips, mut sort_pairs, mut sort_bytes) =
        (0, 0, 0, 0, 0);
    let mut positive_smoke_observations = 0;
    let mut late_sort_allocations = 0;
    let started = Instant::now();
    for frame in 0..case.frames() {
        let before = Instant::now();
        app.update();
        wall.push(before.elapsed().as_secs_f64() * 1000.0);
        // Allow async diagnostics/count delivery; excluded from app.update timings.
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            started.elapsed() < Duration::from_secs(180),
            "live playback timeout"
        );
        assert!(
            !app.world()
                .resource::<CaptureRenderReadiness>()
                .detail
                .starts_with("Composer error")
        );
        for diagnostic in app.world().resource::<DiagnosticsStore>().iter() {
            let path = diagnostic.path().as_str();
            if !path.ends_with("elapsed_gpu") {
                continue;
            }
            if let Some(value) = diagnostic.measurement()
                && value.value.is_finite()
                && seen.get(path).is_none_or(|time| *time < value.time)
            {
                seen.insert(path.to_owned(), value.time);
                metrics
                    .entry(path.to_owned())
                    .or_default()
                    .push(value.value);
            }
        }
        let mut particles = app
            .world_mut()
            .query::<(&PresentedEffect, &GpuParticleStatistics)>();
        let alive: u32 = particles
            .iter(app.world())
            .filter_map(|(effect, stats)| {
                stats
                    .observation(&effect.instance)
                    .map(|(_, counts)| counts.iter().sum::<u32>())
            })
            .sum();
        peak_alive = peak_alive.max(alive);
        let mut timings = app.world_mut().query::<(
            Entity,
            &PresentedEffect,
            &GpuParticleStatistics,
            &GpuSimulationTiming,
        )>();
        for (entity, effect, stats, timing) in timings.iter(app.world()) {
            if let Some(sample) = timing.frame_sample(&effect.instance, stats)
                && sample.requested_time > 0.0
                && sequences
                    .get(&entity)
                    .is_none_or(|sequence| *sequence < sample.sequence)
            {
                sequences.insert(entity, sample.sequence);
                simulation
                    .entry(effect.instance.effect().name.clone())
                    .or_default()
                    .push(sample.nanoseconds as f64 / 1_000_000.0);
                max_fixed_ticks = max_fixed_ticks.max(sample.work.fixed_ticks.unwrap_or(0));
                checkpoint_bytes += sample.work.checkpoint_capture_bytes.unwrap_or(0);
            }
        }
        if matches!(case, Case::Cohorts { .. }) {
            let smoke: u32 = owners
                .iter()
                .filter_map(|owner| {
                    let effect = app.world().get::<PresentedEffect>(*owner)?;
                    app.world()
                        .get::<GpuParticleStatistics>(*owner)?
                        .observation(&effect.instance)
                        .map(|(_, counts)| counts[0])
                })
                .sum();
            peak_smoke = peak_smoke.max(smoke);
            positive_smoke_observations += usize::from(smoke > 0);
        }
        if let Some(profile) = app.world().get::<aestra_bevy::ProjectProfiler>(owner) {
            peak_clips = peak_clips.max(
                profile
                    .0
                    .instances
                    .iter()
                    .filter(|i| !i.path.is_empty())
                    .count(),
            );
        }
        let sort = app.world().resource::<GpuAlphaSortStatistics>().snapshot();
        sort_pairs = sort_pairs.max(sort.pairs);
        sort_bytes = sort_bytes.max(sort.owned_buffer_bytes);
        if frame > 60 {
            late_sort_allocations += sort.allocated_buffers_this_frame;
        }
        assert_native_transport(&app);
    }
    let end_frames: Vec<_> = owners
        .iter()
        .map(|owner| app.world().get::<EffectPlayer>(*owner).unwrap().frame())
        .collect();
    for owner in &owners {
        app.world_mut()
            .get_mut::<EffectPlayer>(*owner)
            .unwrap()
            .playing = false;
    }
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::ZERO,
    ));
    // Drain async admission/statistic messages, not part of live distributions.
    pump(&mut app, 60);
    let mut admission = Vec::new();
    let mut final_populations = Vec::new();
    assert_eq!(
        checkpoint_bytes, 0,
        "playback-only must not capture checkpoints"
    );
    if let Case::Cohorts { roots, draw } = case {
        assert_eq!(end_frames, vec![1080; roots]);
        assert_eq!(peak_smoke, (roots * 64) as u32);
        assert!(positive_smoke_observations > 500);
        for owner in &owners {
            let links = app.world().get::<GpuEventLinkStatistics>(*owner).unwrap();
            assert!(links.readback_samples > 0);
            assert_eq!(links.source_overflow, 0);
            assert_eq!(links.links.len(), 4);
            for link in &links.links {
                assert_eq!(
                    (
                        link.captured_demand,
                        link.accepted,
                        link.expansion_omitted,
                        link.destination_rejected
                    ),
                    (32, 32, 0, 0)
                );
            }
            admission.push(json!({"accepted_by_link":links.links.iter().map(|l|l.accepted).collect::<Vec<_>>(),"source_overflow":links.source_overflow}));
            let effect = app.world().get::<PresentedEffect>(*owner).unwrap();
            let (observed_time, counts) = app
                .world()
                .get::<GpuParticleStatistics>(*owner)
                .unwrap()
                .observation(&effect.instance)
                .unwrap();
            // This must be natural forward-playback cleanup, not a corrective seek,
            // owner retirement, or an image-only assertion about fully faded smoke.
            assert_eq!(effect.instance.time(), 18.0);
            assert_eq!(observed_time, 18.0);
            assert!(
                counts.iter().all(|count| *count == 0),
                "final live populations: {counts:?}"
            );
            final_populations.push(json!({"observed_time":observed_time,"counts":counts,
                "smoke_drained":counts[0] == 0,"playback_time":effect.instance.time()}));
        }
        // Two zero-opacity alpha launch markers remain in both controls.
        assert_eq!(sort_pairs, roots * if draw { 3 } else { 2 });
        assert_eq!(
            late_sort_allocations, 0,
            "capacity-stable sort must reuse buffers"
        );
    } else {
        assert_eq!(
            end_frames,
            vec![if matches!(case, Case::Show) {
                1560
            } else {
                2220
            }]
        );
        assert_eq!(peak_clips, if matches!(case, Case::Show) { 6 } else { 13 });
        assert!(
            peak_alive > 0,
            "must benchmark real child effects, not empty carrier"
        );
    }
    assert!(
        metrics.iter().any(|(path, values)| path
            .ends_with("aestra::bench::full_frame/elapsed_gpu")
            && values.len() > 800),
        "missing fresh render-graph timestamps: {:?}",
        metrics
            .iter()
            .map(|(path, values)| (path, values.len()))
            .collect::<Vec<_>>()
    );
    let adapter = app.world().resource::<GpuCapabilities>();
    let adapter =
        json!({"name":adapter.adapter_name,"backend":adapter.backend,"driver":adapter.driver});
    let selected_cap = app
        .world()
        .resource::<ParticleLightGpuSettings>()
        .max_lights;
    let representative_cap = app.world().resource::<TransientLightSettings>().max_lights;
    let representative = app.world().resource::<TransientLightStatistics>();
    let representative_admission = json!({"accepted":representative.accepted,
        "budget_dropped":representative.budget_dropped,"invalid":representative.invalid,
        "binding_invalid":representative.binding_invalid});
    for owner in owners {
        app.world_mut().despawn(owner);
    }
    pump(&mut app, 30);
    let sort = app.world().resource::<GpuAlphaSortStatistics>().snapshot();
    assert_eq!((sort.pairs, sort.owned_buffer_bytes), (0, 0));
    json!({"case":case.label(),"tier":tier,"adapter":adapter,"updates":case.frames(),
        "end_frames":end_frames,"resolution":[960,540],"fixed_step_hz":60,"seed":"0xf83b000000000001",
        "history":"playback-only","render_gpu_ms":metrics.into_iter().map(|(k,v)|(k,distribution(v))).collect::<BTreeMap<_,_>>(),
        "selected_cap":selected_cap,"representative_cap":representative_cap,"representative_admission":representative_admission,
        "root_scale":if matches!(case,Case::Show | Case::PersistentShow { .. }){1.0}else{0.1},
        "transparent_order":if matches!(case,Case::Show){"fast"}else{"depth-back-to-front"},
        "show_variant":if matches!(case,Case::PersistentShow { .. }){"opt-in saved persistent lit-smoke candidate; representative-only; no HDR/environment/audio; not cross-draw/art/finale acceptance"}else{"historical cohort or unchanged unlit show baseline"},
        "simulation_gpu_ms_by_source":simulation.into_iter().map(|(k,v)|(k,distribution(v))).collect::<BTreeMap<_,_>>(),
        "max_observed_fixed_ticks_per_simulation_window":max_fixed_ticks,"observed_checkpoint_capture_bytes":checkpoint_bytes,
        "app_update_wall_ms":distribution(wall),"peak_async_alive":peak_alive,"peak_async_smoke":peak_smoke,
        "peak_active_clips":peak_clips,"peak_sort_pairs":sort_pairs,"peak_sort_owned_bytes":sort_bytes,
        "late_sort_allocations":late_sort_allocations,"admission":admission,"retired_sort_bytes":sort.owned_buffer_bytes,
        "final_populations":final_populations,
        "scope":"Live forward playback including births and cleanup. Async diagnostic arrivals are deduplicated, not frame-aligned; late paused warmup results can cross the boundary and final live results can arrive after sampling. Do not sum pass percentiles or per-source simulation percentiles. Render-graph scope includes simulation/render GPU work, not preparation, CPU/game/audio or presentation. app.update wall excludes the tooling sleep and is not GPU completion time. See show_variant for unchanged baseline versus migration candidate. Shows use public representative caps without HDR/environment/audio; coincident roots are synthetic cross-draw stress, not approved sorting or choreography."})
}

#[test]
#[ignore = "native F8.1C3 authored persistent-show matched costs; run alone"]
fn persistent_smoke_show_live_matched_draw_costs() {
    let root = PathBuf::from(
        std::env::var_os("AESTRA_PERSISTENT_SHOW_COSTS")
            .expect("use a fresh absolute report directory"),
    );
    assert!(root.is_absolute());
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("accepted.json"), b"{\"accepted\":false}").unwrap();
    for repetition in 1..=2 {
        for tier in ["high", "medium", "low"] {
            for draw in [true, false] {
                let case = Case::PersistentShow { draw };
                let report = run(tier, case);
                fs::write(
                    root.join(format!("{}-{tier}-r{repetition}.json", case.label())),
                    serde_json::to_vec_pretty(&report).unwrap(),
                )
                .unwrap();
            }
        }
    }
    fs::write(
        root.join("accepted.json"),
        b"{\"accepted\":true,\"runs\":12}",
    )
    .unwrap();
}

#[test]
#[ignore = "native F8.1C1 live costs; run alone, two repetitions of all tiers"]
fn live_smoke_overlap_costs_and_authored_show_baseline() {
    let root = std::env::var_os("AESTRA_SMOKE_LIVE_REPORTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fireworks-f8/smoke-live")
        });
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("accepted.json"), b"{\"accepted\":false}").unwrap();
    for repetition in 1..=2 {
        for tier in ["high", "medium", "low"] {
            for case in [
                Case::Cohorts {
                    roots: 1,
                    draw: true,
                },
                Case::Cohorts {
                    roots: 1,
                    draw: false,
                },
                Case::Cohorts {
                    roots: 4,
                    draw: true,
                },
                Case::Cohorts {
                    roots: 4,
                    draw: false,
                },
                Case::Show,
            ] {
                let report = run(tier, case);
                fs::write(
                    root.join(format!("{}-{tier}-r{repetition}.json", case.label())),
                    serde_json::to_vec_pretty(&report).unwrap(),
                )
                .unwrap();
            }
        }
    }
    fs::write(
        root.join("accepted.json"),
        b"{\"accepted\":true,\"runs\":30}",
    )
    .unwrap();
}

#[test]
fn no_draw_control_preserves_authored_smoke_simulation_and_outputs() {
    for tier in ["high", "medium", "low"] {
        let drawn = fixture(
            tier,
            Case::Cohorts {
                roots: 1,
                draw: true,
            },
        );
        let control = fixture(
            tier,
            Case::Cohorts {
                roots: 1,
                draw: false,
            },
        );
        assert_eq!(drawn.root.max_particles, control.root.max_particles);
        assert_eq!(drawn.root.event_links, control.root.event_links);
        assert_eq!(drawn.root.event_routes, control.root.event_routes);
        assert_eq!(drawn.root.point_lights, control.root.point_lights);
        for (a, b) in drawn.root.emitters.iter().zip(&control.root.emitters) {
            assert_eq!(a.max_particles, b.max_particles);
            assert_eq!(a.execution, b.execution);
            assert_eq!(a.scene_outputs, b.scene_outputs);
        }
        assert!(!drawn.root.emitters[0].renderers.is_empty());
        assert!(control.root.emitters[0].renderers.is_empty());
    }
}

#[test]
fn persistent_show_no_draw_control_preserves_all_authored_simulation_and_routes() {
    for tier in ["high", "medium", "low"] {
        let drawn = fixture(tier, Case::PersistentShow { draw: true });
        let control = fixture(tier, Case::PersistentShow { draw: false });
        assert_eq!(drawn.root.effect_clips, control.root.effect_clips);
        for (id, child) in &drawn.dependencies {
            let other = &control.dependencies[id];
            assert_eq!(child.max_particles, other.max_particles);
            assert_eq!(child.event_links, other.event_links);
            assert_eq!(child.event_routes, other.event_routes);
            assert_eq!(child.point_lights, other.point_lights);
            assert_eq!(child.material_instances, other.material_instances);
            let mut removed = 0;
            for (a, b) in child.emitters.iter().zip(&other.emitters) {
                assert_eq!(a.max_particles, b.max_particles);
                assert_eq!(a.execution, b.execution);
                assert_eq!(a.scene_outputs, b.scene_outputs);
                if matches!(a.name.as_str(), "Launch smoke" | "Burst smoke") {
                    assert_eq!(a.renderers.len(), 1);
                    assert!(b.renderers.is_empty());
                    removed += 1;
                } else {
                    assert_eq!(a.renderers, b.renderers);
                }
            }
            assert_eq!(removed, 2);
        }
    }
}
