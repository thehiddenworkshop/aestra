//! F8.1C4L: matched paced forward playback; diagnostic scans are a separate lane.
use super::*;
use aestra_bevy::{
    ExperimentalOitDiagnostics, ExperimentalOitSettings, ExperimentalOitStatistics,
    ExperimentalOitStatus, TransparentOrderMode,
    gpu::{GpuAlphaSortStatistics, GpuParticleStatistics, GpuSimulationTiming},
};
use serde_json::{Value as Json, json};

const START: u64 = 360;
const UPDATES: usize = 240;
const HZ: f64 = 60.0;
const SEED: u64 = 0xf83b_0000_0000_0001;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Sorted,
    DefaultOit,
    ProvisionedOit,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Sorted => "sorted",
            Self::DefaultOit => "oit-default",
            Self::ProvisionedOit => "oit-provisioned",
        }
    }
    fn settings(self) -> Option<ExperimentalOitSettings> {
        match self {
            Self::Sorted => None,
            Self::DefaultOit => Some(ExperimentalOitSettings::default()),
            Self::ProvisionedOit => Some(ExperimentalOitSettings {
                sorted_fragment_max_count: 32,
                max_storage_bytes: 128 * 1024 * 1024,
                ..default()
            }),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Workload {
    Show,
    Dense,
}

impl Workload {
    fn name(self) -> &'static str {
        match self {
            Self::Show => "persistent-show",
            Self::Dense => "four-coincident-cohorts",
        }
    }
    fn fixture(self) -> Arc<aestra_bevy::CompiledEffectProject> {
        live_costs::fixture(
            "high",
            match self {
                Self::Show => live_costs::Case::PersistentShow { draw: true },
                Self::Dense => live_costs::Case::Cohorts {
                    roots: 4,
                    draw: true,
                },
            },
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct Config {
    workload: Workload,
    size: UVec2,
    mode: Mode,
    repetition: usize,
}

impl Config {
    fn name(self) -> String {
        format!(
            "{}-{}x{}-{}-r{}",
            self.workload.name(),
            self.size.x,
            self.size.y,
            self.mode.name(),
            self.repetition
        )
    }
}

fn matrix() -> Vec<Config> {
    let mut cases = Vec::new();
    for repetition in 1..=2 {
        for workload in [Workload::Show, Workload::Dense] {
            for size in [UVec2::new(960, 540), UVec2::new(1920, 1080)] {
                let mut modes = [Mode::Sorted, Mode::DefaultOit, Mode::ProvisionedOit];
                if repetition == 2 {
                    modes.reverse();
                }
                cases.extend(modes.map(|mode| Config {
                    workload,
                    size,
                    mode,
                    repetition,
                }));
            }
        }
    }
    cases
}

fn distribution(mut samples: Vec<f64>) -> Json {
    if samples.is_empty() {
        return Json::Null;
    }
    assert!(samples.iter().all(|v| v.is_finite() && *v >= 0.0));
    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    json!({"samples":n,"median_ms":samples[n/2],"p95_ms":samples[(n-1)*95/100],
        "p99_ms":samples[(n-1)*99/100],"max_ms":samples[n-1]})
}

fn setup(config: Config) -> (App, Vec<Entity>, Entity, Handle<Image>) {
    let compiled = config.workload.fixture();
    let (mut app, owner, target) =
        headless_project_timing("high", true, true, compiled.clone(), true);
    *app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&target)
        .unwrap() = Image::new_target_texture(
        config.size.x,
        config.size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    app.world_mut()
        .resource_mut::<AestraSettings>()
        .transparent_order = TransparentOrderMode::DepthBackToFront;
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera3d>>()
        .single(app.world())
        .unwrap();
    app.world_mut().entity_mut(camera).insert((
        Msaa::Off,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
    ));
    if let Some(settings) = config.mode.settings() {
        app.world_mut().entity_mut(camera).insert(settings);
    }
    let mut owners = vec![owner];
    match config.workload {
        Workload::Show => {
            *app.world_mut().get_mut::<Transform>(owner).unwrap() = Transform::IDENTITY;
            *app.world_mut().get_mut::<Transform>(camera).unwrap() =
                crate::fireworks_show::camera(crate::FireworksCamera::Audience);
            let mut lights = LightingQualityPolicy::preset("high").unwrap();
            lights.particle.enabled = false;
            lights.particle.max_lights = 0;
            lights.apply(app.world_mut()).unwrap();
        }
        Workload::Dense => {
            for _ in 1..4 {
                let mut player = EffectPlayer::from_project(compiled.clone())
                    .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
                player.set_seed(SEED);
                player.playing = false;
                owners.push(
                    app.world_mut()
                        .spawn((player, Transform::from_scale(Vec3::splat(0.1))))
                        .id(),
                );
            }
        }
    }
    (app, owners, camera, target)
}

fn play(app: &mut App, owners: &[Entity], playing: bool) {
    for owner in owners {
        app.world_mut()
            .get_mut::<EffectPlayer>(*owner)
            .unwrap()
            .playing = playing;
    }
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(if playing {
        Duration::from_secs_f64(1.0 / HZ)
    } else {
        Duration::ZERO
    }));
}

fn run(config: Config, diagnostic_lane: bool) -> Json {
    let (mut app, owners, camera, target) = setup(config);
    let _ = capture(&mut app, &target);
    let adapter = app.world().resource::<GpuCapabilities>();
    let adapter =
        json!({"name":adapter.adapter_name,"backend":adapter.backend,"driver":adapter.driver});
    let settings = config.mode.settings().map(|s| {
        json!({"average":s.fragments_per_pixel_average,
        "sorted_max":s.sorted_fragment_max_count,"budget":s.max_storage_bytes})
    });
    let status = app.world().get::<ExperimentalOitStatus>(camera);
    if let Some(rejection) = status.and_then(|s| s.rejection) {
        // A rejected opt-in renders normally: never label its fallback cost as OIT.
        return json!({"name":config.name(),"admitted":false,"rejection":rejection,
            "estimated_bytes":status.unwrap().estimated_storage_bytes,"settings":settings,
            "timing":null,"capacity":null,"adapter":adapter});
    }
    assert_eq!(status.is_some(), config.mode != Mode::Sorted);
    play(&mut app, &owners, true);
    let warmup = Instant::now();
    while app.world().get::<EffectPlayer>(owners[0]).unwrap().frame() < START {
        app.update();
        std::thread::sleep(Duration::from_millis(2));
        assert!(warmup.elapsed() < Duration::from_secs(90));
    }
    play(&mut app, &owners, false);
    if diagnostic_lane {
        assert_ne!(config.mode, Mode::Sorted);
    }
    app.world()
        .resource::<ExperimentalOitDiagnostics>()
        .set_enabled(diagnostic_lane);
    let _ = capture(&mut app, &target);
    pump(&mut app, 60);
    for owner in &owners {
        assert_eq!(
            app.world().get::<EffectPlayer>(*owner).unwrap().frame(),
            START
        );
    }
    let mut seen = BTreeMap::new();
    for d in app.world().resource::<DiagnosticsStore>().iter() {
        if let Some(m) = d.measurement() {
            seen.insert(d.path().as_str().to_owned(), m.time);
        }
    }
    let mut gpu = BTreeMap::<String, Vec<f64>>::new();
    let mut simulations = BTreeMap::<String, Vec<f64>>::new();
    let mut sequences = BTreeMap::new();
    let mut update_ms = Vec::new();
    let mut starts_ms = Vec::new();
    let mut late = 0;
    let mut pending_pipelines = 0;
    let mut alive_peak = 0_u32;
    let mut sort_bytes = 0;
    let mut sort_pairs = 0;
    let mut max_fixed_ticks = 0;
    let mut checkpoint_bytes = 0;
    let mut unknown_checkpoint_windows = 0;
    let mut observations = 0;
    let mut last_sequence = 0;
    let mut max_attempts = 0;
    let mut max_drops = 0;
    let mut max_oversort = 0;
    let mut max_depth = 0;
    let mut dropped_observations = 0;
    let mut oversort_observations = 0;
    play(&mut app, &owners, true);
    let started = Instant::now();
    let mut previous = started;
    for frame in 0..UPDATES {
        let before = Instant::now();
        if frame > 0 {
            starts_ms.push(before.duration_since(previous).as_secs_f64() * 1000.0);
        }
        previous = before;
        app.update();
        update_ms.push(before.elapsed().as_secs_f64() * 1000.0);
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "paced measurement timeout"
        );
        let ready = app.world().resource::<CaptureRenderReadiness>();
        assert!(
            !ready.detail.starts_with("Composer error"),
            "{}",
            ready.detail
        );
        pending_pipelines += usize::from(!ready.ready);
        for d in app.world().resource::<DiagnosticsStore>().iter() {
            let path = d.path().as_str();
            if !path.ends_with("elapsed_gpu") {
                continue;
            }
            if let Some(m) = d.measurement()
                && m.value.is_finite()
                && seen.get(path).is_none_or(|time| *time < m.time)
            {
                seen.insert(path.to_owned(), m.time);
                gpu.entry(path.to_owned()).or_default().push(m.value);
            }
        }
        let mut players = app.world_mut().query::<(
            Entity,
            &PresentedEffect,
            &GpuParticleStatistics,
            &GpuSimulationTiming,
        )>();
        let mut alive = 0;
        for (entity, effect, stats, timing) in players.iter(app.world()) {
            if let Some((_, counts)) = stats.observation(&effect.instance) {
                alive += counts.iter().sum::<u32>();
            }
            if let Some(sample) = timing.frame_sample(&effect.instance, stats)
                && sample.requested_time > START as f32 / HZ as f32
                && sequences
                    .get(&entity)
                    .is_none_or(|sequence| *sequence < sample.sequence)
            {
                sequences.insert(entity, sample.sequence);
                simulations
                    .entry(effect.instance.effect().name.clone())
                    .or_default()
                    .push(sample.nanoseconds as f64 / 1_000_000.0);
                max_fixed_ticks = max_fixed_ticks.max(sample.work.fixed_ticks.unwrap_or(0));
                if let Some(bytes) = sample.work.checkpoint_capture_bytes {
                    checkpoint_bytes += bytes;
                } else {
                    unknown_checkpoint_windows += 1;
                }
            }
        }
        alive_peak = alive_peak.max(alive);
        let sort = app.world().resource::<GpuAlphaSortStatistics>().snapshot();
        sort_bytes = sort_bytes.max(sort.owned_buffer_bytes);
        sort_pairs = sort_pairs.max(sort.pairs);
        let diagnostic = app
            .world()
            .resource::<ExperimentalOitDiagnostics>()
            .snapshot();
        if !diagnostic_lane {
            assert!(diagnostic.is_none());
        }
        if let Some(sample) = diagnostic
            && sample.sequence > last_sequence
        {
            assert!(diagnostic_lane);
            last_sequence = sample.sequence;
            assert_eq!(sample.camera, camera);
            assert_eq!(sample.target_size, config.size);
            assert_eq!(
                sample.invalid_links, 0,
                "invalid lists are not a performance success"
            );
            observations += 1;
            max_attempts = max_attempts.max(sample.attempted_fragments);
            max_drops = max_drops.max(sample.dropped_fragments);
            max_oversort = max_oversort.max(sample.over_sort_pixels);
            max_depth = max_depth.max(sample.capped_max_depth);
            dropped_observations += usize::from(sample.dropped_fragments > 0);
            oversort_observations += usize::from(sample.over_sort_pixels > 0);
        }
        assert_native_transport(&app);
        let deadline = started + Duration::from_secs_f64((frame + 1) as f64 / HZ);
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        } else {
            late += 1;
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    let end_frames: Vec<_> = owners
        .iter()
        .map(|o| app.world().get::<EffectPlayer>(*o).unwrap().frame())
        .collect();
    assert_eq!(end_frames, vec![START + UPDATES as u64; owners.len()]);
    assert!(alive_peak > 0, "empty playback is not a valid benchmark");
    assert_eq!(
        checkpoint_bytes, 0,
        "playback-only must not capture replay checkpoints"
    );
    if diagnostic_lane {
        assert!(
            observations >= UPDATES / 2,
            "insufficient fresh capacity observations: {observations}"
        );
    } else {
        assert!(
            gpu.iter().any(|(path, samples)| path
                .ends_with("aestra::bench::full_frame/elapsed_gpu")
                && samples.len() >= UPDATES / 2),
            "missing whole-frame GPU observations"
        );
    }
    play(&mut app, &owners, false);
    let allocation = app
        .world()
        .resource::<ExperimentalOitStatistics>()
        .snapshot();
    let bytes = allocation.storage_bytes;
    if let Some(settings) = config.mode.settings() {
        assert!(bytes <= settings.max_storage_bytes);
    } else {
        assert!(bytes <= 16);
    }
    json!({"name":config.name(),"admitted":true,"adapter":adapter,"lane":if diagnostic_lane {"capacity scan; not cost acceptance"} else {"cost; OIT diagnostics disabled"},
        "workload":config.workload.name(),"mode":config.mode.name(),"repetition":config.repetition,"resolution":config.size.to_array(),
        "settings":settings,"history":"PlaybackOnly","seed":format!("{SEED:#x}"),"start_frame":START,"updates":UPDATES,"end_frames":end_frames,
        "host_pace_hz":HZ,"elapsed_seconds":elapsed,"late_deadlines":late,"pending_pipeline_updates":pending_pipelines,
        "steady_pipeline_window":pending_pipelines==0,"peak_async_alive":alive_peak,"max_observed_fixed_ticks":max_fixed_ticks,
        "observed_checkpoint_capture_bytes":checkpoint_bytes,"unknown_checkpoint_windows":unknown_checkpoint_windows,
        "allocation":{"nodes":allocation.node_capacity,"heads":allocation.head_capacity,"bytes":bytes},"peak_sort_bytes":sort_bytes,"peak_sort_pairs":sort_pairs,
        "timing":if diagnostic_lane {Json::Null}else{json!({"render_gpu_ms":gpu.into_iter().map(|(k,v)|(k,distribution(v))).collect::<BTreeMap<_,_>>(),
            "simulation_gpu_ms_by_source":simulations.into_iter().map(|(k,v)|(k,distribution(v))).collect::<BTreeMap<_,_>>(),
            "app_update_wall_ms":distribution(update_ms),"update_start_intervals_ms":distribution(starts_ms)})},
        "capacity":if diagnostic_lane {json!({"observations":observations,"max_attempted":max_attempts,"max_dropped":max_drops,"observations_with_drops":dropped_observations,
            "max_over_sort_pixels":max_oversort,"observations_over_sort":oversort_observations,"max_capped_depth":max_depth,"invalid_links":0,
            "observed_exact_capacity":max_drops==0 && max_oversort==0})}else{Json::Null},
        "scope":"6..10 second fixed-step forward window paced at 60 host updates/sec without skips; not display/vsync/game/audio/end-to-end timing. Diagnostics and per-effect timestamp arrivals are deduplicated but not joined to frame IDs; warmup/final results may straddle window boundaries. Never sum pass or source percentiles. Capacity is a separate independently initialized matching playback; no cost-window scan or pixel/particle transport. Ordinary sorted rendering is a cost baseline, not an interleaved-alpha correctness reference. No HDR/bloom or full-finale/art approval."})
}

#[test]
fn paced_oit_matrix_keeps_default_budget_rejection_distinct_from_provisioning() {
    let cases = matrix();
    assert_eq!(cases.len(), 24);
    let default = Mode::DefaultOit.settings().unwrap();
    assert_eq!(default.max_storage_bytes, 32 * 1024 * 1024);
    assert!((960_u64 * 540 * 52) < default.max_storage_bytes);
    assert!((1920_u64 * 1080 * 52) > default.max_storage_bytes);
    assert!((1920_u64 * 1080 * 52) < Mode::ProvisionedOit.settings().unwrap().max_storage_bytes);
    assert!(Mode::Sorted.settings().is_none());
    assert_eq!(cases[0].mode, Mode::Sorted);
    assert_eq!(cases[12].mode, Mode::ProvisionedOit);
    assert_eq!(distribution(vec![3.0, 1.0, 2.0])["median_ms"], 2.0);
    for workload in [Workload::Show, Workload::Dense] {
        let fixture = workload.fixture();
        assert!(!fixture.root.emitters.is_empty() || !fixture.dependencies.is_empty());
    }
}

#[test]
#[ignore = "native paced OIT cost/capacity matrix; run alone, no other GPU workloads"]
fn oit_live_paced_matched_cost_and_capacity() {
    let root = PathBuf::from(
        std::env::var_os("AESTRA_OIT_LIVE_REPORTS").expect("use a fresh absolute report directory"),
    );
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("accepted.json"), b"{\"probe_passed\":false}").unwrap();
    let mut reports = Vec::new();
    for config in matrix() {
        let cost = run(config, false);
        let capacity = if cost["admitted"] == true && config.mode != Mode::Sorted {
            run(config, true)
        } else {
            Json::Null
        };
        let report = json!({"cost":cost,"capacity":capacity});
        fs::write(
            root.join(format!("{}.json", config.name())),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        reports.push(json!({"name":config.name(),"admitted":report["cost"]["admitted"],"capacity":report["capacity"]["capacity"]}));
    }
    fs::write(root.join("accepted.json"),serde_json::to_vec_pretty(&json!({"probe_passed":true,"renderer_adoption":false,"cases":reports,
        "scope":"Benchmark instrumentation/observations complete; rejection and degradation are not OIT acceptance. Public defaults unchanged."})).unwrap()).unwrap();
}
