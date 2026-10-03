//! Native Bevy host consumer: sound selection comes from actual output messages,
//! never an independent sound timer. No audio assets or playback live in Aestra.
use aestra_bevy::gpu::GpuParticleStatistics;
use aestra_bevy::{
    ActiveBackend, AestraOutputEvent, EffectPlayer, EffectRuntimeStatus, EventOrigin,
};
use bevy::{app::AppExit, prelude::*};
use serde::Serialize;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Serialize)]
struct Cue {
    run: u8,
    epoch: u32,
    kind: String,
    host_sound: &'static str,
    tick: u64,
    position_effect_local: [f32; 3],
    particle_count: u32,
}

#[derive(Resource)]
pub struct Check {
    output: PathBuf,
    frames: u32,
    run: u8,
    waiting: bool,
    settled: u32,
    epochs: Vec<u32>,
    seek_tick: u64,
    parent_count: u32,
    secondary_kind: &'static str,
    cues: Vec<Cue>,
    seen: BTreeSet<(u32, String, u64)>,
    stale_ignored: u32,
    done: bool,
}

impl Check {
    pub fn new(output: PathBuf, parent_count: u32) -> Self {
        Self {
            output,
            frames: 0,
            run: 0,
            waiting: true,
            settled: 0,
            epochs: Vec::new(),
            seek_tick: 160,
            parent_count,
            secondary_kind: "secondary_break",
            cues: Vec::new(),
            seen: BTreeSet::new(),
            stale_ignored: 0,
            done: false,
        }
    }

    pub fn with_crackle(mut self) -> Self {
        self.secondary_kind = "crackle";
        self
    }

    pub fn with_crossette(mut self) -> Self {
        self.secondary_kind = "crossette_split";
        self.seek_tick = 133;
        self
    }

    fn hear(&mut self, output: &AestraOutputEvent, epoch: u32) -> Result<(), String> {
        let (emitter, sound) = match output.event.kind.as_str() {
            "launch" => (0, "host/firework_launch"),
            "main_break" => (0, "host/firework_main_break"),
            "secondary_break" => (1, "host/firework_secondary_break"),
            "crackle" => (5, "host/firework_crackle"),
            "crossette_split" => (1, "host/firework_crossette_split"),
            _ => return Ok(()),
        };
        if output.playback_epoch != Some(epoch) {
            self.stale_ignored += 1;
            return Ok(());
        }
        if self.waiting {
            return Err("particle cue during paused initialization/seek reconstruction".into());
        }
        let event = &output.event;
        if event.origin != EventOrigin::Emitter(emitter)
            || event.value.len() != 3
            || event.value.iter().any(|v| !v.is_finite())
            || !event.magnitude.is_finite()
            || event.magnitude < 1.0
            || event.magnitude.fract() != 0.0
            || event.magnitude > self.parent_count as f32
        {
            return Err("invalid particle cue origin/position/count".into());
        }
        if self.run == 1 && event.tick <= self.seek_tick {
            return Err("seek reconstructed an already-past cue".into());
        }
        if !self.seen.insert((epoch, event.kind.clone(), event.tick)) {
            return Err("duplicate host cue in one playback epoch".into());
        }
        self.cues.push(Cue {
            run: self.run,
            epoch,
            kind: event.kind.clone(),
            host_sound: sound,
            tick: event.tick,
            position_effect_local: event.value.as_slice().try_into().unwrap(),
            particle_count: event.magnitude as u32,
        });
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        if self.epochs.len() != 3 || self.epochs.iter().collect::<BTreeSet<_>>().len() != 3 {
            return Err("seek and restart must each start a distinct playback epoch".into());
        }
        for run in [0, 2] {
            let total = |kind: &str| {
                self.cues
                    .iter()
                    .filter(|cue| cue.run == run && cue.kind == kind)
                    .map(|cue| cue.particle_count)
                    .sum::<u32>()
            };
            if [
                total("launch"),
                total("main_break"),
                total(self.secondary_kind),
            ] != [1, 1, self.parent_count]
            {
                return Err(format!(
                    "run {run}: missing or repeated launch/main/secondary demand"
                ));
            }
        }
        let seek: Vec<_> = self.cues.iter().filter(|cue| cue.run == 1).collect();
        let count = seek.iter().map(|cue| cue.particle_count).sum::<u32>();
        if seek
            .iter()
            .any(|cue| cue.kind != self.secondary_kind || cue.tick <= self.seek_tick)
            || !(1..self.parent_count).contains(&count)
        {
            return Err("seek must resume only the remaining secondary breaks".into());
        }
        let original: Vec<_> = self.cues.iter().filter(|cue| cue.run == 0).collect();
        let restarted: Vec<_> = self.cues.iter().filter(|cue| cue.run == 2).collect();
        let remaining: Vec<_> = original
            .iter()
            .copied()
            .filter(|cue| cue.kind == self.secondary_kind && cue.tick > self.seek_tick)
            .collect();
        if remaining.len() != seek.len()
            || remaining.iter().zip(&seek).any(|(a, b)| !same_cue(a, b))
        {
            return Err("resumed seek must deliver the complete remaining live cue stream".into());
        }
        if original.len() != restarted.len()
            || original.iter().zip(restarted).any(|(a, b)| !same_cue(a, b))
        {
            return Err(
                "restart must produce the same new live cue stream in its new epoch".into(),
            );
        }
        Ok(())
    }

    fn finish(&mut self, result: Result<(), String>, exit: &mut MessageWriter<AppExit>) {
        self.done = true;
        let report = serde_json::json!({ "status": if result.is_ok() { "passed" } else { "failed" },
            "error": result.as_ref().err(), "history_policy": "playback-only", "fixed_tick_rate": 60,
            "epochs": self.epochs, "stale_host_messages_ignored": self.stale_ignored, "cues": self.cues,
            "seek_suppressed_through_tick": self.seek_tick, "run": self.run, "waiting": self.waiting,
            "secondary_kind": self.secondary_kind, "expected_parent_count": self.parent_count,
            "scope": "Native GPU root fixture. FirstPerTick coalesces one representative local-space position and particle count per route/tick. Host sound identifiers are bindings only; no audio assets or playback. 32-tick readback ring, not a lossless gameplay bus." });
        let written = (|| -> Result<(), String> {
            if let Some(parent) = self.output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&self.output, serde_json::to_vec_pretty(&report).unwrap())
                .map_err(|e| e.to_string())
        })();
        if let Err(error) = &result {
            eprintln!("fireworks cue check: {error}");
        }
        if let Err(error) = &written {
            eprintln!("fireworks cue report: {error}");
        }
        exit.write(if result.is_ok() && written.is_ok() {
            AppExit::Success
        } else {
            AppExit::error()
        });
    }
}

fn same_cue(a: &Cue, b: &Cue) -> bool {
    a.kind == b.kind
        && a.tick == b.tick
        && a.particle_count == b.particle_count
        && a.position_effect_local
            .iter()
            .zip(b.position_effect_local)
            .all(|(x, y)| (x - y).abs() <= 0.001)
}

pub fn drive(
    mut check: ResMut<Check>,
    mut outputs: MessageReader<AestraOutputEvent>,
    mut players: Query<(
        Entity,
        &mut EffectPlayer,
        Option<&GpuParticleStatistics>,
        Option<&EffectRuntimeStatus>,
    )>,
    mut exit: MessageWriter<AppExit>,
) {
    if check.done {
        return;
    }
    check.frames += 1;
    if check.frames > 3000 {
        check.finish(Err("GPU cue check timed out".into()), &mut exit);
        return;
    }
    let Ok((entity, mut player, statistics, runtime)) = players.single_mut() else {
        return;
    };
    let epoch = player.instance().history_epoch();
    for output in outputs.read().filter(|output| output.effect == entity) {
        if let Err(error) = check.hear(output, epoch) {
            check.finish(Err(error), &mut exit);
            return;
        }
    }
    let Some((observed_time, _)) = statistics.and_then(|s| s.observation(player.instance())) else {
        return;
    };
    if runtime.is_none_or(|status| status.active != ActiveBackend::Gpu) {
        check.finish(Err("cue check did not use native GPU".into()), &mut exit);
        return;
    }
    if check.waiting {
        let target =
            aestra_bevy::trace_tick(player.instance().history_epoch_start_time()) as f32 / 60.0;
        if (observed_time - target).abs() < 0.001 {
            check.settled += 1;
            if check.settled >= 20 {
                check.waiting = false;
                check.settled = 0;
                check.epochs.push(epoch);
                player.playing = true;
            }
        }
        return;
    }
    // The production GPU's fixed-tick floor can finish one tick below an
    // accumulated f32 end time. All cue activity ended several seconds earlier.
    if player.frame() < 420 || observed_time < 419.0 / 60.0 - 0.001 {
        return;
    }
    // Let asynchronous readbacks drain before changing epochs.
    check.settled += 1;
    if check.settled < 20 {
        return;
    }
    if check.run == 2 {
        let result = check.validate();
        check.finish(result, &mut exit);
        return;
    }
    check.run += 1;
    check.waiting = true;
    check.settled = 0;
    if check.run == 1 {
        player.seek_frame(check.seek_tick);
        check.seek_tick = aestra_bevy::trace_tick(player.instance().history_epoch_start_time());
    } else {
        player.restart();
    }
    player.playing = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cue_check_requires_native_playback_only_and_a_fresh_report() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cues.json");
        let args = vec![
            "--fireworks-f0".into(),
            "--fireworks-f0-probe".into(),
            "f5-multi-break".into(),
            "--backend".into(),
            "gpu".into(),
            "--history".into(),
            "playback-only".into(),
            "--fireworks-cue-check".into(),
            path.to_string_lossy().into_owned(),
        ];
        let parse = |args: Vec<String>| super::super::ViewerConfig::from_iter(args);
        assert!(parse(args.clone()).is_ok());
        let mut replay = args.clone();
        replay[6] = "replay-enabled".into();
        assert!(parse(replay).is_err());
        let mut cpu = args.clone();
        cpu[4] = "cpu".into();
        assert!(parse(cpu).is_err());
        let mut bench = args.clone();
        bench.extend(["--gpu-bench".into(), "unused.json".into()]);
        assert!(parse(bench).is_err());
        let mut existing = args;
        existing[8] = temp.path().to_string_lossy().into_owned();
        assert!(parse(existing).is_err());
    }

    #[test]
    fn host_rejects_stale_epochs_and_duplicate_cues() {
        let mut check = Check::new("unused.json".into(), 64);
        check.waiting = false;
        let cue = AestraOutputEvent::root(
            Entity::PLACEHOLDER,
            aestra_bevy::EffectOutputEvent::new(
                "launch",
                EventOrigin::Emitter(0),
                "",
                vec![0.0; 3],
                1.0,
                1,
            ),
        )
        .in_epoch(2);
        check.hear(&cue, 3).unwrap();
        assert!(check.cues.is_empty());
        check.hear(&cue, 2).unwrap();
        assert!(check.hear(&cue, 2).is_err());
        assert_eq!(check.cues[0].host_sound, "host/firework_launch");
    }
}
