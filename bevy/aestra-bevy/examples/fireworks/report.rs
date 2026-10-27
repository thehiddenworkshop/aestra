//! Optional real-window qualification hook. No viewer implementation or fixed-time replay.
use super::{AudienceCamera, ShowRoot, oit::Admission, project::Options, readiness::Readiness};
use aestra_bevy::{
    ActiveBackend, AestraRuntimeStatus, EffectPlayer, ExperimentalOitDiagnostics,
    ExperimentalOitStatistics, ExperimentalOitStatus, PresentedEffect, TransientLightStatistics,
    gpu::{GpuParticleStatistics, GpuPlaybackDiagnostics},
};
use bevy::{
    camera::Hdr,
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};
use serde_json::json;
use std::{path::PathBuf, time::Instant};
#[path = "report_profile.rs"]
mod profile;

#[derive(Resource)]
pub(super) struct Report {
    path: PathBuf,
    images: bool,
    intervals: Vec<f64>,
    previous: Option<Instant>,
    wall_start: Option<Instant>,
    real_seconds: f64,
    game_seconds: f64,
    paused_updates: usize,
    pending: usize,
    cleanup_frames: usize,
    captures: Vec<serde_json::Value>,
    captured: usize,
    live_peak: u32,
    last_live: u32,
    observed_owners: usize,
    last_sequence: u64,
    observations: usize,
    max_drops: u32,
    max_invalid: u32,
    max_over_sort: u32,
    playback_sequence: u64,
    playback_frames: usize,
    blocked_owner_frames: usize,
    encoded_owner_frames: usize,
    max_encoding_lag: f32,
    worst_encoding_lag: serde_json::Value,
    max_schedule_error: f32,
    missing_scheduled_owner_frames: usize,
}

pub fn install(app: &mut App) -> Result<(), String> {
    let Some(path) = std::env::var_os("AESTRA_EXAMPLE_REPORT") else {
        return Ok(());
    };
    if std::env::var_os("AESTRA_EXAMPLE_CAPTURE").is_some() {
        return Err("Use either the full-show report or the single-capture hook".into());
    }
    let path = PathBuf::from(path);
    if !path.is_absolute() || path.exists() {
        return Err("AESTRA_EXAMPLE_REPORT must be a fresh absolute file path".into());
    }
    std::fs::create_dir_all(path.parent().ok_or("Report parent missing")?)
        .map_err(|e| e.to_string())?;
    let images = std::env::var("AESTRA_EXAMPLE_REPORT_IMAGES").is_ok_and(|v| v == "1");
    let profiling = std::env::var("AESTRA_EXAMPLE_PROFILE").is_ok_and(|v| v == "1");
    if profiling && images {
        return Err("Profiling and image/capacity scans require separate runs".into());
    }
    if profiling {
        profile::install(app);
    }
    app.world()
        .resource::<GpuPlaybackDiagnostics>()
        .set_enabled(true);
    std::fs::write(&path, b"{\"completed\":false}").map_err(|e| e.to_string())?;
    // Capacity scans and screenshots belong to the separate image lane, never cost.
    if images && app.world().resource::<Options>().experimental_oit {
        app.world()
            .resource::<ExperimentalOitDiagnostics>()
            .set_enabled(true);
    }
    app.insert_resource(Report {
        path,
        images,
        intervals: Vec::new(),
        previous: None,
        wall_start: None,
        real_seconds: 0.0,
        game_seconds: 0.0,
        paused_updates: 0,
        pending: 0,
        cleanup_frames: 0,
        captures: Vec::new(),
        captured: 0,
        live_peak: 0,
        last_live: 0,
        observed_owners: 0,
        last_sequence: 0,
        observations: 0,
        max_drops: 0,
        max_invalid: 0,
        max_over_sort: 0,
        playback_sequence: 0,
        playback_frames: 0,
        blocked_owner_frames: 0,
        encoded_owner_frames: 0,
        max_encoding_lag: 0.0,
        worst_encoding_lag: serde_json::Value::Null,
        max_schedule_error: 0.0,
        missing_scheduled_owner_frames: 0,
    })
    .add_systems(Update, record.after(aestra_bevy::AestraSet::Playback));
    Ok(())
}

fn distribution(samples: &[f64]) -> serde_json::Value {
    if samples.is_empty() {
        return serde_json::Value::Null;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    json!({"samples":n,"median_ms":sorted[n/2],"p95_ms":sorted[(n-1)*95/100],
        "p99_ms":sorted[(n-1)*99/100],"max_ms":sorted[n-1],
        "over_16_67ms":samples.iter().filter(|v| **v > 1000.0/60.0).count(),
        "over_20ms":samples.iter().filter(|v| **v > 20.0).count(),
        "over_33_33ms":samples.iter().filter(|v| **v > 1000.0/30.0).count()})
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn record(
    mut report: ResMut<Report>,
    options: Res<Options>,
    readiness: Res<Readiness>,
    admission: Res<Admission>,
    root: Query<&EffectPlayer, With<ShowRoot>>,
    probe: (
        Query<(&PresentedEffect, &GpuParticleStatistics)>,
        Query<(&PresentedEffect, &aestra_bevy::EffectClipInstance)>,
        Res<GpuPlaybackDiagnostics>,
        Option<Res<profile::Profile>>,
    ),
    camera: Query<
        (
            &Camera,
            Option<&ExperimentalOitStatus>,
            Has<Hdr>,
            Option<&Tonemapping>,
            Option<&Bloom>,
            Option<&Msaa>,
            Option<&aestra_bevy::EffectPipelinePreparation>,
        ),
        With<AudienceCamera>,
    >,
    runtime: Res<AestraRuntimeStatus>,
    capabilities: Res<aestra_bevy::GpuCapabilities>,
    lights: Res<TransientLightStatistics>,
    allocation: Res<ExperimentalOitStatistics>,
    diagnostics: Res<ExperimentalOitDiagnostics>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    time: Res<Time<Real>>,
    game_time: Res<Time>,
) {
    let (effects, clips, playback, profile) = probe;
    if time.elapsed_secs() > 110.0 {
        error!("Full-show report timed out");
        exit.write(AppExit::error());
        return;
    }
    if !readiness.started {
        return;
    }
    let Ok(player) = root.single() else { return };
    let frame = playback.snapshot();
    if frame.sequence > report.playback_sequence {
        report.playback_sequence = frame.sequence;
        report.playback_frames += 1;
        for sample in frame.samples {
            if let Some(encoded) = sample.encoded_time {
                report.encoded_owner_frames += 1;
                let lag = sample.requested_time - encoded;
                if lag > report.max_encoding_lag {
                    report.max_encoding_lag = lag;
                    report.worst_encoding_lag = json!({"effect":effects.get(sample.owner).ok().map(|(e,_)|e.effect().name.clone()),"requested_seconds":sample.requested_time,"encoded_seconds":encoded});
                }
            } else {
                report.blocked_owner_frames += 1;
            }
        }
    }
    if let Some(project) = player.project() {
        for scheduled in project.instances_with(
            player.instance().time(),
            player.instance().seed(),
            player.instance().host_transform_context(),
            |_, clip| Some(clip.clone()),
        ) {
            if scheduled.path.is_empty() {
                continue;
            }
            if let Some((effect, _)) = clips.iter().find(|(_, clip)| clip.path == scheduled.path) {
                report.max_schedule_error = report
                    .max_schedule_error
                    .max((effect.simulation_time() - scheduled.time).abs());
            } else {
                report.missing_scheduled_owner_frames += 1;
            }
        }
    }
    if player.elapsed() < player.effect().duration {
        let now = Instant::now();
        report.wall_start.get_or_insert(now);
        report.real_seconds += time.delta_secs_f64();
        report.game_seconds += game_time.delta_secs_f64();
        report.paused_updates += usize::from(!player.playing);
        if let Some(previous) = report.previous {
            report
                .intervals
                .push(now.duration_since(previous).as_secs_f64() * 1000.0);
        }
        report.previous = Some(now);
        report.pending += usize::from(!readiness.pipelines_ready());
    } else {
        report.cleanup_frames += 1;
    }
    let mut alive = 0;
    let mut owners = 0;
    for (effect, stats) in &effects {
        if let Some((_, counts)) = stats.observation(&effect.instance) {
            alive += counts.iter().sum::<u32>();
            owners += 1;
        }
    }
    report.live_peak = report.live_peak.max(alive);
    report.last_live = alive;
    report.observed_owners = owners;
    if let Some(sample) = diagnostics.snapshot()
        && sample.sequence > report.last_sequence
    {
        report.last_sequence = sample.sequence;
        report.observations += 1;
        report.max_drops = report.max_drops.max(sample.dropped_fragments);
        report.max_invalid = report.max_invalid.max(sample.invalid_links);
        report.max_over_sort = report.max_over_sort.max(sample.over_sort_pixels);
    }
    let samples = [8.0, 15.0, 17.45, 25.0, 32.0];
    let index = report.captures.len();
    if report.images && index < samples.len() && player.elapsed() >= samples[index] {
        let name = format!(
            "{}-{}s.png",
            report.path.file_stem().unwrap().to_string_lossy(),
            samples[index]
        );
        let path = report.path.with_file_name(name);
        report.captures.push(json!({"path":path,"nominal_seconds":samples[index],"request_seconds":player.elapsed(),"delivered":false}));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path))
            .observe(
                move |_: On<ScreenshotCaptured>, mut report: ResMut<Report>| {
                    report.captured += 1;
                    report.captures[index]["delivered"] = json!(true);
                },
            );
    }
    if report.cleanup_frames < 10 || report.captured < report.captures.len() {
        return;
    }
    let Ok((camera, status, hdr, tonemapping, bloom, msaa, preparation)) = camera.single() else {
        return;
    };
    let allocation = allocation.snapshot();
    let capacity = (report.images && options.experimental_oit).then(|| json!({
        "observations":report.observations,"max_dropped":report.max_drops,"max_invalid_links":report.max_invalid,
        "max_over_sort_pixels":report.max_over_sort,"loss_free":report.observations>0 && report.max_drops==0 && report.max_invalid==0}));
    let encoding_ledger = json!({"frames":report.playback_frames,"encoded_owner_frames":report.encoded_owner_frames,"blocked_owner_frames":report.blocked_owner_frames,"max_requested_minus_encoded_seconds":report.max_encoding_lag,"max_clip_schedule_error_seconds":report.max_schedule_error,
        "missing_scheduled_owner_frames":report.missing_scheduled_owner_frames,"worst_lag":report.worst_encoding_lag,
        "scope":"CPU command encoding only; asynchronous latest frame, no GPU completion fence or visible-pixel assertion. Stateful quantization up to one simulation tick is expected."});
    let mut output = json!({"completed":true,"adoption_accepted":false,
        "lane":if report.images {"images-and-capacity"} else {"host-pacing-no-scans-no-screenshots"},
        "experimental_oit":options.experimental_oit,"tier":options.tier,"history":format!("{:?}", options.history),
        "effect":options.effect_path,"seed":super::project::SHOW_SEED,"backend":format!("{:?}",runtime.active),
        "native_gpu":runtime.active==ActiveBackend::Gpu,"final_seconds":player.elapsed(),"duration":player.effect().duration,
        "wall_playback_seconds":report.wall_start.map(|t|t.elapsed().as_secs_f64()),"bevy_real_seconds":report.real_seconds,"bevy_game_seconds":report.game_seconds,"paused_updates":report.paused_updates,"player_speed":player.speed,
        "adapter":{"name":capabilities.adapter_name,"backend":capabilities.backend,"driver":capabilities.driver},
        "host_intervals_including_vsync":distribution(&report.intervals),"pending_pipeline_updates":report.pending,
        "camera_physical_size":camera.physical_target_size().map(|s| [s.x,s.y]),
        "hdr":hdr,"tonemapping":tonemapping.map(|v|format!("{v:?}")),"bloom_intensity":bloom.map(|b|b.intensity),"msaa":msaa.map(|v|format!("{v:?}")),
        "oit_estimated_bytes":status.map(|s|s.estimated_storage_bytes),"oit_rejection":status.and_then(|s|s.rejection),
        "node_head_allocated_bytes":allocation.storage_bytes,"capacity":capacity,
        "static_renderers":admission.renderers,"static_sprites":admission.sprites,"static_trails":admission.trails,
        "peak_async_alive":report.live_peak,"last_async_alive":report.last_live,"last_observed_owners":report.observed_owners,
        "flashes_accepted":lights.accepted,"flashes_peak":lights.peak_active,
        "captures":report.captures,"audio_enabled":options.audio_root.is_some(),
        "scope":"Actual public-window HDR/bloom/environment show. Real-time forward playback, asynchronous screenshots/counts. Image lane adds readback/scans and is not a cost lane. Host intervals are not GPU completion or exact display-present timestamps. No automatic adoption or photographic approval."});
    output["encoding_ledger"] = encoding_ledger;
    output["prepared_view"] = json!(preparation.map(|p| format!("{:?}", p.status())));
    if let Some(profile) = profile {
        output["lane"] = json!("profiling-timestamps-no-images");
        output["profile"] = profile.summary();
    }
    if let Err(error) = std::fs::write(&report.path, serde_json::to_vec_pretty(&output).unwrap()) {
        error!("Cannot write fireworks report: {error}");
        exit.write(AppExit::error());
    } else {
        info!("Full-show report saved: {}", report.path.display());
        exit.write(AppExit::Success);
    }
}
