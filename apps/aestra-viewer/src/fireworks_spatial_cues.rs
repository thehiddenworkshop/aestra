//! F6B host-side acceptance of native cues from the reusable show.
//! Sound names are bindings only; no audio assets, mixer or scripted sound timer.
use aestra_bevy::gpu::GpuParticleStatistics;
use aestra_bevy::{
    ActiveBackend, AestraOutputEvent, CompiledEffectProject, EffectClipId, EffectClipInstance,
    EffectPlayer, EffectRuntimeStatus, EventOrigin, PresentedEffect,
};
use bevy::{app::AppExit, prelude::*};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

const SEEK: f32 = 18.0;

pub fn placement() -> Transform {
    Transform {
        translation: Vec3::new(12.0, 3.0, -7.0),
        rotation: Quat::from_rotation_y(0.35),
        scale: Vec3::new(0.9, 1.0, 0.8),
    }
}

#[derive(Clone, Serialize)]
struct Cue {
    run: u8,
    epoch: u32,
    clip: String,
    source: String,
    seed: u64,
    kind: String,
    host_sound: String,
    tick: u64,
    root_time_seconds: f32,
    position_effect_local: [f32; 3],
    position_world: [f32; 3],
    particle_count: u32,
}

#[derive(Resource)]
pub struct Check {
    output: PathBuf,
    project: Arc<CompiledEffectProject>,
    seed: u64,
    run: u8,
    frames: u32,
    waiting: bool,
    settled: u32,
    epochs: Vec<u32>,
    cues: Vec<Cue>,
    seen: BTreeSet<(u32, EffectClipId, String, u64)>,
    stale_ignored: u32,
    done: bool,
    lights: Option<serde_json::Value>,
}

impl Check {
    pub fn new(output: PathBuf, project: Arc<CompiledEffectProject>, seed: u64) -> Self {
        Self {
            output,
            project,
            seed,
            run: 0,
            frames: 0,
            waiting: true,
            settled: 0,
            epochs: vec![],
            cues: vec![],
            seen: BTreeSet::new(),
            stale_ignored: 0,
            done: false,
            lights: None,
        }
    }

    fn hear(&mut self, output: &AestraOutputEvent, epoch: u32) -> Result<(), String> {
        if output.particle.is_none() && output.event.kind == aestra_bevy::EVENT_FINISHED {
            return Ok(());
        }
        if output.playback_epoch != Some(epoch) {
            self.stale_ignored += 1;
            return Ok(());
        }
        let [id] = output.clip_path.as_slice() else {
            return Err("native show cue lost its stable clip path".into());
        };
        let clip = self
            .project
            .root
            .effect_clips
            .iter()
            .find(|c| c.source_clip == *id)
            .ok_or("unknown source clip")?;
        let source = self
            .project
            .effect(clip.source.id)
            .ok_or("missing source effect")?;
        let route = source
            .particle_outputs()
            .map(|(_, route)| route)
            .find(|route| route.output == output.event.kind)
            .ok_or("unknown particle cue route")?;
        let spatial = output
            .particle
            .as_ref()
            .ok_or("particle cue has no spatial/source metadata")?;
        let time = clip.start_time - clip.source_offset + output.event.tick as f32 / 60.0;
        let local: [f32; 3] = output
            .event
            .value
            .as_slice()
            .try_into()
            .map_err(|_| "missing local position")?;
        let world = spatial.world_position.ok_or("missing world position")?;
        let clip_matrix = Mat4::from_scale_rotation_translation(
            Vec3::from_array(clip.transform.scale),
            Quat::from_array(clip.transform.rotation),
            Vec3::from_array(clip.transform.translation),
        );
        let expected =
            (placement().to_matrix() * clip_matrix).transform_point3(Vec3::from_array(local));
        if spatial.source_effect != source.source
            || spatial.seed != clip.seed.resolve(self.seed, *id)
            || output.event.origin != EventOrigin::Emitter(route.source)
            || !spatial.root_time_seconds.is_finite()
            || !local.iter().chain(world.iter()).all(|v| v.is_finite())
            || (spatial.root_time_seconds - time).abs() > 0.00001
            || !Vec3::from_array(world).abs_diff_eq(expected, 0.001)
            || !output.event.magnitude.is_finite()
            || output.event.magnitude < 1.0
            || output.event.magnitude.fract() != 0.0
            || output.event.magnitude > source.max_particles as f32
        {
            return Err("invalid source/seed/origin/time/position/count in host cue".into());
        }
        if self.run == 1 && time <= SEEK + 0.00001 {
            return Err("seek announced a reconstructed, already-past cue".into());
        }
        if !self
            .seen
            .insert((epoch, *id, output.event.kind.clone(), output.event.tick))
        {
            return Err("duplicate cue for the same root epoch, clip, route and tick".into());
        }
        let sound = match output.event.kind.as_str() {
            "launch" => "shell_launch",
            "main_break" => "shell_burst",
            "secondary_break" => "secondary_burst",
            "crackle" => "crackle",
            "crossette_split" => "crossette_split",
            _ => return Err("unbound sound route".into()),
        };
        self.cues.push(Cue {
            run: self.run,
            epoch,
            clip: id.to_string(),
            source: source.source.to_string(),
            seed: spatial.seed,
            kind: output.event.kind.clone(),
            host_sound: sound.into(),
            tick: output.event.tick,
            root_time_seconds: time,
            position_effect_local: local,
            position_world: world,
            particle_count: output.event.magnitude as u32,
        });
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        if self.epochs.len() != 3 || self.epochs.iter().collect::<BTreeSet<_>>().len() != 3 {
            return Err("seek and restart must have distinct root epochs".into());
        }
        for run in [0, 2] {
            for clip in &self.project.root.effect_clips {
                let source = self.project.effect(clip.source.id).unwrap();
                let route_names: BTreeSet<_> = source
                    .particle_outputs()
                    .map(|(_, route)| route.output.as_str())
                    .collect();
                if !["launch", "main_break"]
                    .iter()
                    .all(|name| route_names.contains(name))
                {
                    return Err("every shell source must export launch and main_break".into());
                }
                let counts: BTreeMap<_, _> = source
                    .particle_outputs()
                    .map(|(_, route)| {
                        let expected = match route.output.as_str() {
                            "launch" | "main_break" => 1,
                            _ => source.event_links[0].count,
                        };
                        (route.output.as_str(), expected)
                    })
                    .collect();
                for (kind, expected) in counts {
                    let total: u32 = self
                        .cues
                        .iter()
                        .filter(|cue| {
                            cue.run == run
                                && cue.clip == clip.source_clip.to_string()
                                && cue.kind == kind
                        })
                        .map(|cue| cue.particle_count)
                        .sum();
                    if total != expected {
                        return Err(format!(
                            "run {run}, clip {}, {kind}: received {total}, expected {expected}",
                            clip.source_clip
                        ));
                    }
                }
            }
        }
        let sorted = |run, after| {
            let mut cues: Vec<_> = self
                .cues
                .iter()
                .filter(|cue| cue.run == run && cue.root_time_seconds > after)
                .collect();
            cues.sort_by(|a, b| (&a.clip, &a.kind, a.tick).cmp(&(&b.clip, &b.kind, b.tick)));
            cues
        };
        for (run, after) in [(1, SEEK + 0.00001), (2, -1.0)] {
            let original = sorted(0, after);
            let observed = sorted(run, -1.0);
            if original.is_empty()
                || original.len() != observed.len()
                || original.iter().zip(observed).any(|(a, b)| !same_cue(a, b))
            {
                return Err(format!(
                    "run {run}: missing/different resumed or restarted cue stream"
                ));
            }
        }
        Ok(())
    }

    fn finish(&mut self, result: Result<(), String>, exit: &mut MessageWriter<AppExit>) {
        self.done = true;
        let report = serde_json::json!({ "status": if result.is_ok() { "passed" } else { "failed" },
            "error": result.as_ref().err(), "history_policy": "playback-only", "fixed_tick_rate": 60,
            "epochs": self.epochs, "seek_seconds": SEEK, "stale_host_messages_ignored": self.stale_ignored,
            "clip_count": self.project.root.effect_clips.len(), "run": self.run,
            "frames": self.frames, "waiting": self.waiting, "cues": self.cues,
            "transient_lights": self.lights,
            "scope": "13 repeated/transformed clips, static nonidentity ECS root placement; historical authored transforms are unit-tested. FirstPerTick coalescing, 32-tick ring, not lossless gameplay delivery. Host sound names only; no audio playback. Arbitrarily moving ECS placement is delivery-time, not recorded history." });
        let written = (|| -> Result<(), String> {
            if let Some(parent) = self.output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&self.output, serde_json::to_vec_pretty(&report).unwrap())
                .map_err(|e| e.to_string())
        })();
        if let Err(error) = &result {
            eprintln!("spatial cue check: {error}");
        }
        if let Err(error) = &written {
            eprintln!("spatial cue report: {error}");
        }
        exit.write(if result.is_ok() && written.is_ok() {
            AppExit::Success
        } else {
            AppExit::error()
        });
    }
}

fn same_cue(a: &Cue, b: &Cue) -> bool {
    a.clip == b.clip
        && a.source == b.source
        && a.seed == b.seed
        && a.kind == b.kind
        && a.tick == b.tick
        && a.particle_count == b.particle_count
        && a.position_effect_local
            .iter()
            .zip(b.position_effect_local)
            .all(|(x, y)| (x - y).abs() <= 0.001)
        && a.position_world
            .iter()
            .zip(b.position_world)
            .all(|(x, y)| (x - y).abs() <= 0.001)
}

pub fn drive(
    mut check: ResMut<Check>,
    mut outputs: MessageReader<AestraOutputEvent>,
    mut players: Query<(Entity, &mut EffectPlayer)>,
    children: Query<(
        &EffectClipInstance,
        &PresentedEffect,
        Option<&GpuParticleStatistics>,
        Option<&EffectRuntimeStatus>,
    )>,
    mut exit: MessageWriter<AppExit>,
    light_stats: Option<Res<aestra_bevy::TransientLightStatistics>>,
    light_settings: Option<Res<aestra_bevy::TransientLightSettings>>,
) {
    if check.done {
        return;
    }
    check.frames += 1;
    if let (Some(stats), Some(settings)) = (&light_stats, &light_settings) {
        check.lights = Some(serde_json::json!({
            "budget": settings.max_lights, "allocated": stats.allocated,
            "active": stats.active, "peak_active": stats.peak_active,
            "accepted": stats.accepted, "budget_dropped": stats.budget_dropped,
            "duplicate": stats.duplicate, "invalid": stats.invalid,
            "stale": stats.stale, "expired": stats.expired, "disabled": stats.disabled,
            "binding_invalid": stats.binding_invalid, "source_packets_dropped": stats.source_packets_dropped,
            "scope": "Opt-in authored representative-light bindings, shadowless pooled lights. Not lit smoke."
        }));
    }
    if check.frames > 9000 {
        check.finish(Err("spatial GPU cue check timed out".into()), &mut exit);
        return;
    }
    let Ok((entity, mut player)) = players.single_mut() else {
        return;
    };
    let epoch = player.instance().history_epoch();
    for output in outputs.read() {
        // Legacy presentation completion is not one of the selected particle
        // routes. Child completion remains a separate notification contract.
        if output.particle.is_none() && output.event.kind == aestra_bevy::EVENT_FINISHED {
            continue;
        }
        // Detect the original bug instead of silently filtering child-owned cues.
        if output.effect != entity {
            check.finish(
                Err("show output addressed a transient child, not its root".into()),
                &mut exit,
            );
            return;
        }
        if let Err(error) = check.hear(output, epoch) {
            check.finish(Err(error), &mut exit);
            return;
        }
    }
    if check.waiting {
        let expected = check
            .project
            .instances(player.instance().time(), check.seed);
        let ready = expected
            .iter()
            .filter(|i| !i.path.is_empty())
            .all(|scheduled| {
                children
                    .iter()
                    .find(|(clip, _, _, _)| clip.root == entity && clip.path == scheduled.path)
                    .is_some_and(|(_, presented, stats, runtime)| {
                        runtime.is_some_and(|r| r.active == ActiveBackend::Gpu)
                            && stats
                                .and_then(|s| s.observation(&presented.instance))
                                .is_some_and(|(time, _)| {
                                    (time - aestra_bevy::trace_tick(scheduled.time) as f32 / 60.0)
                                        .abs()
                                        < 0.001
                                })
                    })
            });
        if ready {
            check.settled += 1;
        } else {
            check.settled = 0;
        }
        if check.settled >= 20 {
            check.waiting = false;
            check.settled = 0;
            check.epochs.push(epoch);
            player.playing = true;
        }
        return;
    }
    if player.instance().time() < check.project.root.duration - 0.001 {
        return;
    }
    player.playing = false;
    check.settled += 1;
    if check.settled < 60 {
        return;
    }
    if children.iter().any(|(clip, _, _, _)| clip.root == entity) {
        check.finish(
            Err("finished show left clip presentations alive".into()),
            &mut exit,
        );
        return;
    }
    if check.run == 2 {
        let result = check.validate().and_then(|()| {
            if let (Some(stats), Some(settings)) = (&light_stats, &light_settings) {
                let requested = check.cues.iter().filter(|c| c.kind == "main_break").count() as u64;
                if stats.accepted == 0
                    || stats.accepted + stats.budget_dropped + stats.expired != requested
                    || stats.active != 0
                    || stats.allocated > settings.max_lights
                    || stats.peak_active > settings.max_lights
                    || stats.invalid + stats.stale + stats.duplicate + stats.disabled != 0
                    || stats.binding_invalid + stats.source_packets_dropped != 0
                {
                    return Err(format!(
                        "light admission/cleanup failed for {requested} burst cues: {stats:?}"
                    ));
                }
            }
            Ok(())
        });
        check.finish(result, &mut exit);
        return;
    }
    check.run += 1;
    check.waiting = true;
    check.settled = 0;
    if check.run == 1 {
        player.seek_simulation_time(SEEK);
    } else {
        player.restart();
    }
    player.playing = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Check {
        let source = crate::fireworks_show::effect();
        let index = aestra_project::ProjectAssetIndex::scan(crate::viewer_asset_root(None));
        let resolved = index.resolve_effect_project(&source).unwrap();
        let project = aestra_bevy::EffectCompiler::default()
            .compile_resolved_project(&resolved)
            .unwrap();
        Check::new(
            "unused.json".into(),
            Arc::new(project),
            crate::fireworks_f0::SEED,
        )
    }
    fn cue(check: &Check, clip_index: usize) -> AestraOutputEvent {
        let clip = &check.project.root.effect_clips[clip_index];
        let source = check.project.effect(clip.source.id).unwrap();
        let event = aestra_bevy::EffectOutputEvent::new(
            "launch",
            EventOrigin::Emitter(0),
            "",
            vec![0.0; 3],
            1.0,
            1,
        );
        let matrix = placement().to_matrix()
            * Mat4::from_scale_rotation_translation(
                Vec3::from_array(clip.transform.scale),
                Quat::from_array(clip.transform.rotation),
                Vec3::from_array(clip.transform.translation),
            );
        let mut output = AestraOutputEvent::root(Entity::PLACEHOLDER, event).in_epoch(2);
        output.clip_path = vec![clip.source_clip];
        output.particle = Some(aestra_bevy::ParticleOutputContext {
            source_effect: source.source,
            seed: clip.seed.resolve(check.seed, clip.source_clip),
            root_time_seconds: clip.start_time + 1.0 / 60.0,
            world_position: Some(matrix.transform_point3(Vec3::ZERO).to_array()),
        });
        output
    }
    #[test]
    fn show_checker_requires_native_playback_only_and_a_fresh_path() {
        let temp = tempfile::tempdir().unwrap();
        let args: Vec<String> = [
            "--fireworks-f0",
            "--fireworks-f0-probe",
            "f6-show",
            "--backend",
            "gpu",
            "--history",
            "playback-only",
            "--fireworks-cue-check",
        ]
        .map(String::from)
        .into_iter()
        .chain([temp
            .path()
            .join("spatial.json")
            .to_string_lossy()
            .into_owned()])
        .collect();
        assert!(crate::ViewerConfig::from_iter(args.clone()).is_ok());
        let mut replay = args.clone();
        replay[6] = "replay-enabled".into();
        assert!(crate::ViewerConfig::from_iter(replay).is_err());
        let mut cpu = args.clone();
        cpu[4] = "cpu".into();
        assert!(crate::ViewerConfig::from_iter(cpu).is_err());
        let mut exists = args;
        exists[8] = temp.path().to_string_lossy().into_owned();
        assert!(crate::ViewerConfig::from_iter(exists).is_err());
    }

    #[test]
    fn host_checks_path_seed_spatial_context_duplicates_stale_epochs_and_seek_boundary() {
        let mut check = fixture();
        let output = cue(&check, 0);
        check.hear(&output, 3).unwrap();
        assert!(check.cues.is_empty());
        for invalid in [0, 1, 2, 3, 4] {
            let mut bad = output.clone();
            match invalid {
                0 => bad.clip_path.clear(),
                1 => bad.particle.as_mut().unwrap().seed += 1,
                2 => bad.particle.as_mut().unwrap().world_position = None,
                3 => bad.particle.as_mut().unwrap().world_position = Some([0.0; 3]),
                _ => bad.particle.as_mut().unwrap().root_time_seconds = f32::NAN,
            }
            assert!(check.hear(&bad, 2).is_err());
        }
        check.hear(&output, 2).unwrap();
        assert!(check.hear(&output, 2).is_err());
        // Same kind/tick in another occurrence is independent, not a duplicate.
        let other = cue(&check, 4);
        check.hear(&other, 2).unwrap();
        assert_eq!(check.cues.len(), 2);
        check.run = 1;
        assert!(check.hear(&cue(&check, 3), 2).is_err());
        assert!(
            check.validate().is_err(),
            "missing native cohorts must fail"
        );
    }
}
