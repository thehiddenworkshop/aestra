//! Host-side loading only. Shipping applications can load an offline compiled artifact instead.
use aestra_bevy::{
    CompiledEffectProject, EffectAsset, EffectCompiler, LightingQualityPolicy,
    PlaybackHistoryPolicy, QualityTier,
};
use aestra_project::ProjectAssetIndex;
use bevy::prelude::Resource;
use std::{path::PathBuf, sync::Arc};

pub const SHOW_SEED: u64 = 0xf1e0_0000_0000_0001;
pub const USAGE: &str = "cargo run --release -p aestra-bevy --example fireworks -- [--tier high|medium|low] [--history playback-only|replay-enabled] [--project ASSET_ROOT] [--effect PATH] [--audio-root WAV_FOLDER | --no-audio]\nEnable local WAV sound with --features fireworks-audio. Space pauses, R restarts, 1/2/3 changes camera, L toggles lights, M mutes, Esc exits.";

#[derive(Resource)]
pub struct ShowProject(pub Arc<CompiledEffectProject>);

#[derive(Resource)]
pub struct Options {
    pub tier: String,
    pub history: PlaybackHistoryPolicy,
    pub project_root: PathBuf,
    pub effect_path: PathBuf,
    pub audio_root: Option<PathBuf>,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            tier: "high".into(),
            history: PlaybackHistoryPolicy::PlaybackOnly,
            project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test"),
            effect_path: "effects/fireworks_show.aestra.ron".into(),
            audio_root: if cfg!(feature = "fireworks-audio") {
                Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/audio/fireworks"))
            } else {
                None
            },
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if arg == "--no-audio" {
                options.audio_root = None;
                continue;
            }
            let value = args
                .next()
                .ok_or_else(|| format!("Missing value for {arg}"))?;
            match arg.as_str() {
                "--tier" if LightingQualityPolicy::preset(&value).is_some() => options.tier = value,
                "--history" => {
                    options.history = match value.as_str() {
                        "playback-only" => PlaybackHistoryPolicy::PlaybackOnly,
                        "replay-enabled" => PlaybackHistoryPolicy::ReplayEnabled,
                        _ => return Err("History must be playback-only or replay-enabled".into()),
                    }
                }
                "--project" => options.project_root = value.into(),
                "--effect" => options.effect_path = value.into(),
                "--audio-root" if cfg!(feature = "fireworks-audio") => {
                    options.audio_root = Some(value.into())
                }
                "--audio-root" => {
                    return Err("Rebuild with --features fireworks-audio for WAV playback".into());
                }
                _ => return Err(format!("Unknown option or invalid value: {arg} {value}")),
            }
        }
        options.project_root = options
            .project_root
            .canonicalize()
            .map_err(|e| format!("Project root: {e}"))?;
        if !options.project_root.is_dir() {
            return Err("Project root must be a directory".into());
        }
        if !options.effect_path.is_absolute() {
            options.effect_path = options.project_root.join(&options.effect_path);
        }
        Ok(options)
    }

    pub fn compile(&self) -> Result<Arc<CompiledEffectProject>, String> {
        let source = std::fs::read_to_string(&self.effect_path)
            .map_err(|e| format!("{}: {e}", self.effect_path.display()))?;
        let effect = EffectAsset::from_ron(&source).map_err(|e| e.to_string())?;
        let resolved = ProjectAssetIndex::scan(&self.project_root)
            .resolve_effect_project(&effect)
            .map_err(|e| e.to_string())?;
        let project = EffectCompiler::default()
            .with_tier(QualityTier::preset(&self.tier).expect("validated tier"))
            .compile_resolved_project(&resolved)
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(project))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::{EffectPlaybackMode, EffectPlayer};

    #[test]
    fn defaults_use_public_playback_only_and_authored_show() {
        let options = Options::parse([]).unwrap();
        assert_eq!(options.history, PlaybackHistoryPolicy::PlaybackOnly);
        assert!(
            options
                .effect_path
                .ends_with("effects/fireworks_show.aestra.ron")
        );
        let replay = Options::parse(["--history".into(), "replay-enabled".into()]).unwrap();
        assert_eq!(replay.history, PlaybackHistoryPolicy::ReplayEnabled);
        for args in [
            vec!["--tier", "ultra"],
            vec!["--history", "yes"],
            vec!["--effect"],
        ] {
            assert!(Options::parse(args.into_iter().map(String::from)).is_err());
        }
    }

    #[test]
    fn all_tiers_resolve_shared_shells_materials_cues_and_lights_without_viewer() {
        let mut previous = u64::MAX;
        for tier in ["high", "medium", "low"] {
            let options = Options::parse(["--tier".into(), tier.into()]).unwrap();
            let project = options.compile().unwrap();
            assert_eq!(project.root.duration, 26.0);
            assert_eq!(project.root.playback_mode, EffectPlaybackMode::Once);
            assert_eq!(project.root.effect_clips.len(), 13);
            assert_eq!(project.dependencies.len(), 4);
            for shell in project.dependencies.values() {
                assert!(!shell.material_programs.is_empty());
                assert!(!shell.material_instances.is_empty());
                let presented = aestra_bevy::PresentedEffect::new(shell.clone());
                for emitter in &shell.emitters {
                    for renderer in &emitter.renderers {
                        assert!(
                            presented
                                .material_binding_for_emitter(renderer.material, emitter.source)
                                .is_some()
                        );
                    }
                }
                assert_eq!(shell.point_lights.len(), 1);
                let routes: Vec<_> = shell
                    .particle_outputs()
                    .map(|(_, route)| route.output.as_str())
                    .collect();
                assert!(routes.contains(&"launch") && routes.contains(&"main_break"));
            }
            let capacity = project
                .dependencies
                .values()
                .map(|s| s.max_particles as u64)
                .sum();
            assert!(capacity < previous);
            previous = capacity;
            let player = EffectPlayer::from_project(project).with_history_policy(options.history);
            assert_eq!(player.history_policy(), PlaybackHistoryPolicy::PlaybackOnly);
        }
    }
}
