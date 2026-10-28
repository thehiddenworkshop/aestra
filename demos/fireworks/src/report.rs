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
#[path = "report_camera.rs"]
mod camera;
#[path = "report_outputs.rs"]
mod outputs;
#[path = "report_profile.rs"]
mod profile;

#[derive(Resource)]
pub(super) struct Report {
    path: PathBuf,
    images: bool,
    camera_name: String,
    review_camera: camera::ReviewCamera,
    msaa_off: bool,
    star_smoke_visible: bool,
    representative_flashes_visible: bool,
    sample_times: Vec<f32>,
    star_paths: std::collections::BTreeMap<String, serde_json::Value>,
    event_links: std::collections::BTreeMap<String, serde_json::Value>,
    event_links_truncated: bool,
    output_audit: outputs::OutputAudit,
    intervals: Vec<f64>,
    phase_intervals: std::collections::BTreeMap<&'static str, Vec<f64>>,
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
    max_attempts: u32,
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
    selected_peak_capacity: u32,
    selected_rejected_updates: usize,
    selected_last_rejection: Option<String>,
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
    let camera_name =
        std::env::var("AESTRA_EXAMPLE_REPORT_CAMERA").unwrap_or_else(|_| "host-default".into());
    let review_camera = camera::ReviewCamera::parse(&camera_name, images)?;
    let msaa_off = msaa_control(std::env::var("AESTRA_EXAMPLE_REPORT_MSAA").ok().as_deref())?;
    let star_smoke_visible = match std::env::var("AESTRA_EXAMPLE_REPORT_STAR_SMOKE").as_deref() {
        Ok("0") if images => false,
        Err(_) | Ok("1") => true,
        _ => {
            return Err(
                "STAR_SMOKE=0 requires the separate image lane; otherwise omit or use 1".into(),
            );
        }
    };
    let representative_flashes_visible = flash_control(
        std::env::var("AESTRA_EXAMPLE_REPORT_FLASHES")
            .ok()
            .as_deref(),
        images,
    )?;
    let sample_times = if let Ok(times) = std::env::var("AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES") {
        let times = times
            .split(',')
            .map(|s| s.trim().parse::<f32>().map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        if !images
            || times.is_empty()
            || times
                .iter()
                .any(|t| !t.is_finite() || *t <= 0.0 || *t > 37.0)
            || times.windows(2).any(|w| w[0] >= w[1])
        {
            return Err("Image sample times must be finite, increasing and within (0,37]".into());
        }
        times
    } else {
        vec![8.0, 15.0, 17.45, 25.0, 32.0]
    };
    if profiling {
        profile::install(app);
    }
    apply_flash_control(
        &mut app
            .world_mut()
            .resource_mut::<aestra_bevy::TransientLightSettings>(),
        representative_flashes_visible,
    );
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
        camera_name,
        review_camera,
        msaa_off,
        star_smoke_visible,
        representative_flashes_visible,
        sample_times,
        star_paths: default(),
        event_links: default(),
        event_links_truncated: false,
        output_audit: default(),
        intervals: Vec::new(),
        phase_intervals: default(),
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
        max_attempts: 0,
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
        selected_peak_capacity: 0,
        selected_rejected_updates: 0,
        selected_last_rejection: None,
    })
    .add_systems(Startup, review_camera_setup.after(super::setup))
    .add_systems(
        Update,
        camera::motion
            .after(aestra_bevy::AestraSet::Playback)
            .before(aestra_bevy::AestraRenderSet::Prepare),
    )
    .add_systems(
        Update,
        star_smoke_control
            .after(aestra_bevy::AestraSet::Playback)
            .before(aestra_bevy::AestraRenderSet::Prepare),
    )
    .add_systems(
        Update,
        record
            .after(camera::motion)
            .after(aestra_bevy::AestraSet::Playback),
    );
    Ok(())
}

fn review_camera_setup(
    report: Res<Report>,
    mut cameras: Query<(Entity, &mut Transform, &mut GlobalTransform), With<AudienceCamera>>,
    mut commands: Commands,
) {
    if report.msaa_off {
        for (entity, _, _) in &mut cameras {
            commands.entity(entity).insert(Msaa::Off);
        }
    }
    if let Some(transform) = report.review_camera.pose(0.0) {
        for (entity, mut camera, mut global) in &mut cameras {
            *camera = transform;
            *global = GlobalTransform::from(transform);
            commands.entity(entity).insert(report.review_camera);
        }
    }
}

fn flash_control(value: Option<&str>, images: bool) -> Result<bool, String> {
    match value {
        None | Some("1") => Ok(true),
        Some("0") if images => Ok(false),
        _ => Err("FLASHES=0 requires the separate image lane; otherwise omit or use 1".into()),
    }
}

fn msaa_control(value: Option<&str>) -> Result<bool, String> {
    match value {
        None => Ok(false),
        Some("off") => Ok(true),
        _ => {
            Err("REPORT_MSAA must be omitted or 'off' for an explicit matched-view control".into())
        }
    }
}

fn apply_flash_control(settings: &mut aestra_bevy::TransientLightSettings, visible: bool) {
    // Zero intensity preserves admission, pooled proxies and event routes. Disabling
    // the adapter would remove accepted flashes and change the control workload.
    if !visible {
        settings.max_lumens = 0.0;
    }
}

fn star_smoke_control(
    report: Res<Report>,
    mut effects: Query<(Entity, &mut PresentedEffect)>,
    mut applied: Local<std::collections::BTreeSet<Entity>>,
) {
    if report.star_smoke_visible {
        return;
    }
    applied.retain(|owner| effects.contains(*owner));
    for (owner, mut effect) in &mut effects {
        if applied.contains(&owner) {
            continue;
        }
        let materials: Vec<_> = effect
            .effect()
            .emitters
            .iter()
            .filter(|e| e.name == "Star path smoke")
            .flat_map(|e| e.renderers.iter().map(|r| (e.source, r.material)))
            .collect();
        for (source, material) in materials {
            let mut binding = effect
                .material_binding_for_emitter(material, source)
                .expect("authored star material")
                .clone();
            let density = binding
                .program()
                .reflection
                .parameters
                .iter()
                .find(|p| p.name == "Density")
                .expect("smoke density")
                .id;
            binding
                .set_value(density, aestra_bevy::material::MaterialValue::Float(0.0))
                .expect("float density");
            effect.bind_material(material, binding);
        }
        applied.insert(owner);
    }
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

fn observed_phase<'a>(
    options: &Options,
    host: &super::host::HostShow,
    effects: impl Iterator<Item = (&'a PresentedEffect, &'a GpuParticleStatistics)>,
) -> &'static str {
    if !options.host_driven {
        return "authored";
    }
    if host.measuring_launches(options) {
        return "launch-and-overlap";
    }
    if !host.has_vfx() {
        return "idle";
    }
    for (effect, stats) in effects {
        if effect.effect().emitters.is_empty() {
            continue;
        }
        let Some((_, counts)) = stats.observation(&effect.instance) else {
            return "active-fireworks";
        };
        if effect
            .effect()
            .emitters
            .iter()
            .zip(counts)
            .any(|(e, n)| *n > 0 && !e.name.to_ascii_lowercase().contains("smoke"))
        {
            return "active-fireworks";
        }
    }
    "smoke-drain"
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn record(
    mut report: ResMut<Report>,
    options: Res<Options>,
    readiness: Res<Readiness>,
    admission: Res<Admission>,
    root: Query<&EffectPlayer, With<ShowRoot>>,
    probe: (
        Query<(Entity, &PresentedEffect, &GpuParticleStatistics)>,
        Query<(&PresentedEffect, &aestra_bevy::EffectClipInstance)>,
        Res<GpuPlaybackDiagnostics>,
        Option<Res<profile::Profile>>,
        Query<(
            Entity,
            &PresentedEffect,
            &aestra_bevy::gpu::GpuEventLinkStatistics,
        )>,
        Query<&PointLight, With<aestra_bevy::TransientLightProxy>>,
        MessageReader<aestra_bevy::AestraOutputEvent>,
        Option<Res<aestra_bevy::ParticleLightGpuStatistics>>,
        Res<super::host::HostShow>,
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
            &Transform,
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
    let (
        effects,
        clips,
        playback,
        profile,
        links,
        flash_proxies,
        mut output_events,
        selected,
        host,
    ) = probe;
    if time.elapsed_secs() > 110.0 {
        error!("Full-show report timed out");
        exit.write(AppExit::error());
        return;
    }
    if !readiness.started {
        return;
    }
    let Ok(player) = root.single() else { return };
    let selected = selected.map(|stats| stats.snapshot());
    if let Some(stats) = &selected {
        report.selected_peak_capacity = report.selected_peak_capacity.max(stats.written_capacity);
        if let Some(reason) = &stats.rejection {
            report.selected_rejected_updates += 1;
            report.selected_last_rejection =
                Some(format!("{reason:?}").chars().take(512).collect());
        }
    }
    for event in output_events.read() {
        let now = if options.host_driven {
            effects
                .get(event.effect)
                .ok()
                .map(|(_, effect, _)| effect.instance.time())
                .unwrap_or(player.instance().time())
        } else {
            player.instance().time()
        };
        report.output_audit.observe(event, now);
    }
    for (owner, effect, stats) in &links {
        for (index, link) in effect.effect().event_links.iter().enumerate() {
            let star_path = effect.effect().emitters[link.target].name == "Star path smoke";
            // Extra link records belong to image/admission review; preserve the
            // ordinary pacing lane's existing bookkeeping workload.
            if !report.images && !star_path {
                continue;
            }
            if let Some(counts) = stats.links.get(index) {
                let value = json!({"effect":effect.effect().name,
                    "source":effect.effect().emitters[link.source].name,
                    "target":effect.effect().emitters[link.target].name,
                    "readback_samples":stats.readback_samples,"source_overflow":stats.source_overflow,
                    "counts":{"captured_demand":counts.captured_demand,"accepted":counts.accepted,
                        "expansion_omitted":counts.expansion_omitted,"destination_rejected":counts.destination_rejected}});
                if star_path {
                    report
                        .star_paths
                        .insert(format!("{owner:?}"), value.clone());
                }
                if report.images {
                    let key = format!("{owner:?}/{index}");
                    if report.event_links.len() < 256 || report.event_links.contains_key(&key) {
                        report.event_links.insert(key, value);
                    } else {
                        report.event_links_truncated = true;
                    }
                }
            }
        }
    }
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
                    report.worst_encoding_lag = json!({"effect":effects.get(sample.owner).ok().map(|(_,e,_)|e.effect().name.clone()),"requested_seconds":sample.requested_time,"encoded_seconds":encoded});
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
            let ms = now.duration_since(previous).as_secs_f64() * 1000.0;
            report.intervals.push(ms);
            let phase = observed_phase(&options, &host, effects.iter().map(|(_, e, s)| (e, s)));
            report.phase_intervals.entry(phase).or_default().push(ms);
        }
        report.previous = Some(now);
        report.pending += usize::from(!readiness.pipelines_ready());
    } else {
        report.cleanup_frames += 1;
    }
    let mut alive = 0;
    let mut owners = 0;
    for (_, effect, stats) in &effects {
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
        report.max_attempts = report.max_attempts.max(sample.attempted_fragments);
        report.max_drops = report.max_drops.max(sample.dropped_fragments);
        report.max_invalid = report.max_invalid.max(sample.invalid_links);
        report.max_over_sort = report.max_over_sort.max(sample.over_sort_pixels);
    }
    let index = report.captures.len();
    if report.images
        && let Some(sample) = report.sample_times.get(index).copied()
        && player.elapsed() >= sample
    {
        let name = format!(
            "{}-{}s.png",
            report.path.file_stem().unwrap().to_string_lossy(),
            sample
        );
        let path = report.path.with_file_name(name);
        let flash_peak = flash_proxies
            .iter()
            .map(|p| p.intensity)
            .fold(0.0, f32::max);
        // Capture asynchronous per-owner populations while clips still exist,
        // so a quiet image and eventual owner retirement cannot hide a live tail.
        let populations: Vec<_> = effects
            .iter()
            .filter_map(|(owner, effect, stats)| {
                let (observed_source_seconds, counts) = stats.observation(&effect.instance)?;
                Some(
                    json!({"owner":format!("{owner:?}"),"effect":effect.effect().name,"effect_id":effect.effect().source,
                "source_seconds":effect.instance.time(),"observed_source_seconds":observed_source_seconds,
                "emitters":effect.effect().emitters.iter().zip(counts).map(|(emitter, count)|
                    json!({"name":emitter.name,"alive":count})).collect::<Vec<_>>()}),
                )
            })
            .collect();
        let view = camera.single().ok().map(|(_, _, _, _, _, _, _, pose)| json!({"translation":pose.translation.to_array(),"rotation_xyzw":pose.rotation.to_array()}));
        report.captures.push(json!({"path":path,"nominal_seconds":sample,"request_seconds":player.elapsed(),"request_camera_pose":view,"delivered":false,
            "representative_proxy_peak_lumens":flash_peak,"representative_active":lights.active,
            "representative_accepted":lights.accepted,"asynchronous_populations":populations,
            "selected_lights":selected.as_ref().map(selected_observation),
            "host_state":options.host_driven.then(||host.summary())}));
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
    let Ok((camera, status, hdr, tonemapping, bloom, msaa, preparation, _)) = camera.single()
    else {
        return;
    };
    let allocation = allocation.snapshot();
    let capacity = (report.images && options.experimental_oit).then(|| json!({
        "observations":report.observations,"max_attempted_fragments":report.max_attempts,"max_dropped":report.max_drops,"max_invalid_links":report.max_invalid,
        "max_over_sort_pixels":report.max_over_sort,"loss_free":report.observations>0 && report.max_drops==0 && report.max_invalid==0}));
    let encoding_ledger = json!({"frames":report.playback_frames,"encoded_owner_frames":report.encoded_owner_frames,"blocked_owner_frames":report.blocked_owner_frames,"max_requested_minus_encoded_seconds":report.max_encoding_lag,"max_clip_schedule_error_seconds":report.max_schedule_error,
        "missing_scheduled_owner_frames":report.missing_scheduled_owner_frames,"worst_lag":report.worst_encoding_lag,
        "scope":"CPU command encoding only; asynchronous latest frame, no GPU completion fence or visible-pixel assertion. Stateful quantization up to one simulation tick is expected."});
    let mut output = json!({"completed":true,"adoption_accepted":false,
        "lane":if report.images {"images-and-capacity"} else {"host-pacing-no-scans-no-screenshots"},
        "experimental_oit":options.experimental_oit,"tier":options.tier,"history":format!("{:?}", options.history),
        "effect":(!options.host_driven).then_some(&options.effect_path),"seed":super::project::SHOW_SEED,"backend":format!("{:?}",runtime.active),
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
    output["fragment_capacity_passed"] = if report.images && options.experimental_oit {
        json!(report.observations > 0 && report.max_drops == 0 && report.max_invalid == 0)
    } else {
        serde_json::Value::Null
    };
    output["oit_health_scope"] = json!(
        "Interactive experimental OIT enables async capacity/integrity scans and stops after observed fragment loss. Report lanes deliberately finish failed qualifications; completed is not acceptance. Only image reports scan fragment lists; cost/profile lanes omit that health-scan overhead and do not measure the entire interactive guarded-demo cost."
    );
    output["host_phase_intervals_including_vsync"] = json!(
        report
            .phase_intervals
            .iter()
            .map(|(phase, samples)| (*phase, distribution(samples)))
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    output["phase_scope"] = json!(
        "Main-world observation phase: launch-and-overlap until final host detonation; active-fireworks while a non-smoke emitter is alive or unobserved; smoke-drain while only retained smoke/tail owners remain; idle after owner retirement. Counts are asynchronous; boundary intervals are tagged at observation, not GPU execution."
    );
    output["host_driven"] = json!(options.host_driven);
    output["tuned_smoke"] = json!(options.tuned_smoke);
    output["launch_pattern"] = json!(options.launch_pattern);
    if options.host_driven {
        output["host_state"] = host.summary();
    }
    output["review_camera"] = json!(report.camera_name);
    output["requested_msaa_off"] = json!(report.msaa_off);
    output["oit_fragments_per_pixel"] = json!(options.oit_fragments_per_pixel);
    output["review_camera_scope"] = json!(
        "Image orbit reads the root clock after Playback, before render preparation/transform propagation. Capture poses are request-time local transforms of the unparented camera; image and GPU observations arrive asynchronously. Constant 180-unit horizontal radius, y=48, target=(0,28,0), yaw=30-75*cos(2*pi*t/16) degrees. No lens, effect, seek or history changes; fixed views ignore the orbit formula."
    );
    output["star_smoke_visible"] = json!(report.star_smoke_visible);
    output["representative_flashes_visible"] = json!(report.representative_flashes_visible);
    output["flash_rejections"] = json!({"budget_dropped":lights.budget_dropped,
        "expired":lights.expired,"invalid":lights.invalid,"disabled":lights.disabled});
    output["flash_binding"] = json!({"packets_seen":lights.source_packets_seen,
        "scan_dropped":lights.source_packets_dropped,"non_particle":lights.source_non_particle,
        "missing_root":lights.source_missing_root,"epoch_mismatch":lights.source_epoch_mismatch,
        "unbound":lights.source_unbound,"invalid":lights.binding_invalid,"bound":lights.source_bound,
        "stale_intents":lights.stale,"duplicates":lights.duplicate});
    output["particle_output_audit"] = report.output_audit.summary();
    output["selected_lighting_enabled"] = json!(options.lighting_policy().particle.enabled);
    output["representative_lighting_enabled"] =
        json!(options.lighting_policy().representative.enabled);
    output["selected_lighting"] = json!({"peak_capacity_bound":report.selected_peak_capacity,
        "rejected_updates":report.selected_rejected_updates,"last_rejection":report.selected_last_rejection,
        "final":selected.as_ref().map(selected_observation),
        "scope":"Asynchronous native adapter capacity/dispatch metadata, not live or illuminated particle counts. No selected-position readback."});
    output["star_path_admission"] = json!(report.star_paths);
    output["event_link_admission"] = json!(report.event_links);
    output["event_link_admission_truncated"] = json!(report.event_links_truncated);
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

fn selected_observation(stats: &aestra_bevy::ParticleLightGpuObservation) -> serde_json::Value {
    json!({"capacity_bound":stats.written_capacity,"reserved_slots":stats.reserved_slots,
        "dispatches":stats.dispatches,"sequence":stats.sequence,"buffer_bytes":stats.buffer_bytes,
        "invalid_sources":stats.invalid_sources,"rejection":stats.rejection.as_ref().map(|v|format!("{v:?}"))})
}

#[cfg(test)]
#[path = "report_control_tests.rs"]
mod tests;
