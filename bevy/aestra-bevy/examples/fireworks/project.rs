//! Host-side loading only. Shipping applications can load an offline compiled artifact instead.
use aestra_bevy::{
    CompiledEffectProject, EffectAsset, EffectCompiler, ExtensionRegistry, LightingQualityPolicy,
    PlaybackHistoryPolicy, QualityTier,
};
use aestra_project::ProjectAssetIndex;
use bevy::prelude::Resource;
use std::{path::PathBuf, sync::Arc};

pub const SHOW_SEED: u64 = 0xf1e0_0000_0000_0001;
pub const USAGE: &str = "cargo run --release -p aestra-bevy --example fireworks -- [--smoke-lighting | --particle-smoke-lighting] [--tier high|medium|low] [--history playback-only|replay-enabled] [--project ASSET_ROOT] [--effect PATH] [--audio-root WAV_FOLDER | --no-audio]\n--smoke-lighting plays the saved F8.3B fluid fixture; --particle-smoke-lighting plays F8.3C lit sprites (neither is the full show). Enable local WAV sound with --features fireworks-audio. Space pauses, R restarts, 1/2/3 changes camera, L toggles lights, M mutes, Esc exits.";

#[derive(Resource)]
pub struct ShowProject(pub Arc<CompiledEffectProject>);

#[derive(Resource)]
pub struct Options {
    pub tier: String,
    pub history: PlaybackHistoryPolicy,
    pub project_root: PathBuf,
    pub effect_path: PathBuf,
    pub audio_root: Option<PathBuf>,
    pub smoke_lighting: bool,
    pub particle_smoke_lighting: bool,
}

impl Options {
    pub fn lighting_policy(&self) -> LightingQualityPolicy {
        let mut policy = LightingQualityPolicy::preset(&self.tier).unwrap();
        policy.particle.enabled = self.smoke_lighting;
        if self.smoke_lighting {
            // Two saved outputs at 4/2/1 lights each; no 96-slot reservation for an 8-light lab.
            policy.particle.max_lights = match self.tier.as_str() {
                "high" => 8,
                "medium" => 4,
                _ => 2,
            };
            policy.representative.max_lights = 2;
        }
        policy
    }

    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            tier: "high".into(),
            smoke_lighting: false,
            particle_smoke_lighting: false,
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
        let mut explicit_effect = false;
        let mut fluid_lab = false;
        while let Some(arg) = args.next() {
            if arg == "--smoke-lighting" {
                options.smoke_lighting = true;
                fluid_lab = true;
                continue;
            }
            if arg == "--particle-smoke-lighting" {
                options.smoke_lighting = true;
                options.particle_smoke_lighting = true;
                continue;
            }
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
                "--effect" => {
                    options.effect_path = value.into();
                    explicit_effect = true;
                }
                "--audio-root" if cfg!(feature = "fireworks-audio") => {
                    options.audio_root = Some(value.into())
                }
                "--audio-root" => {
                    return Err("Rebuild with --features fireworks-audio for WAV playback".into());
                }
                _ => return Err(format!("Unknown option or invalid value: {arg} {value}")),
            }
        }
        if options.smoke_lighting && !explicit_effect {
            options.effect_path = if options.particle_smoke_lighting {
                "effects/fireworks_particle_smoke_lighting.aestra.ron"
            } else {
                "effects/fireworks_smoke_lighting.aestra.ron"
            }
            .into();
        }
        if fluid_lab && options.particle_smoke_lighting {
            return Err("Choose either --smoke-lighting or --particle-smoke-lighting".into());
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
        let mut registry = if self.smoke_lighting {
            ExtensionRegistry::builtin()
        } else {
            ExtensionRegistry::linked()
        };
        if self.smoke_lighting && !self.particle_smoke_lighting {
            registry
                .install(&aestra_fluid::FluidExtension)
                .map_err(|e| e.to_string())?;
        }
        let project = EffectCompiler::with_extensions(registry)
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
    fn saved_smoke_fixture_compiles_real_outputs_at_each_tier() {
        for (tier, cap) in [("high", 4), ("medium", 2), ("low", 1)] {
            let options =
                Options::parse(["--smoke-lighting".into(), "--tier".into(), tier.into()]).unwrap();
            let project = options.compile().unwrap();
            let root = &project.root;
            assert_eq!(options.lighting_policy().particle.max_lights, cap * 2);
            assert_eq!(root.point_lights.len(), 2);
            assert_eq!(root.particle_outputs().count(), 2);
            assert_eq!(root.emitters.len(), 2);
            for emitter in &root.emitters {
                assert!(
                    emitter.renderers.is_empty(),
                    "only smoke may contribute pixels"
                );
                let aestra_runtime::SceneOutputPlanKind::ParticlePointLight(light) =
                    &emitter.scene_outputs[0].kind;
                assert_eq!(light.max_lights, cap);
            }
        }
    }

    #[test]
    fn lit_particle_smoke_resolves_opt_in_material_and_real_outputs() {
        for tier in ["high", "medium", "low"] {
            let options = Options::parse([
                "--particle-smoke-lighting".into(),
                "--tier".into(),
                tier.into(),
            ])
            .unwrap();
            let project = options.compile().unwrap();
            let root = &project.root;
            assert!(
                root.extension_stages.is_empty(),
                "no fluid can counterfeit sprite response"
            );
            assert_eq!(root.emitters.len(), 3);
            assert_eq!(root.emitters[0].renderers.len(), 1);
            assert!(root.emitters[1..].iter().all(|e| e.renderers.is_empty()));
            assert_eq!(root.point_lights.len(), 2);
            assert_eq!(root.particle_outputs().count(), 2);
            let presented = aestra_bevy::PresentedEffect::new(root.clone());
            let binding = presented
                .material_binding_for_emitter(
                    root.emitters[0].renderers[0].material,
                    root.emitters[0].source,
                )
                .unwrap();
            assert!(binding.program().requires_scene_lighting());
            assert!(!binding.program().requires_scene_depth());
        }
        assert!(
            Options::parse([
                "--smoke-lighting".into(),
                "--particle-smoke-lighting".into()
            ])
            .is_err()
        );
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
