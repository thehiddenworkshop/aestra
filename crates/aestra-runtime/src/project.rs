//! Shared deterministic instance scheduling for editor and engine hosts.
use crate::{
    CompiledEffect, CompiledEffectClip, CompiledEffectProject, CompiledParameterOverride,
    HostTransformContext, InheritedHostTransform,
};
use aestra_core::{EffectClipId, EffectPlaybackMode};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ScheduledEffectInstance {
    /// Stable identity within one root player (empty for the root).
    pub path: Vec<EffectClipId>,
    pub effect: Arc<CompiledEffect>,
    pub time: f32,
    pub seed: u64,
    pub inherited: Arc<InheritedHostTransform>,
    pub parameter_overrides: Vec<CompiledParameterOverride>,
}

impl CompiledEffectProject {
    pub fn instances(&self, time: f32, seed: u64) -> Vec<ScheduledEffectInstance> {
        let time = if self.root.playback_mode == EffectPlaybackMode::LoopRestart {
            time.rem_euclid(self.root.duration)
        } else {
            time
        };
        self.instances_with(
            time,
            seed,
            HostTransformContext {
                motion: self.root.host_transform_track.clone(),
                inherited: Arc::default(),
            },
            |_, clip| Some(clip.clone()),
        )
    }

    /// Host policy may suppress a clip/subtree or preview an edited clip. `path`
    /// identifies the parent. The root context allows runtime motion overrides.
    /// `time` is the host's simulation time; an explicit seek to the last frame
    /// of restart playback is preserved, while child scheduling uses loop phase.
    pub fn instances_with(
        &self,
        time: f32,
        seed: u64,
        root_context: HostTransformContext,
        mut resolve_clip: impl FnMut(&[EffectClipId], &CompiledEffectClip) -> Option<CompiledEffectClip>,
    ) -> Vec<ScheduledEffectInstance> {
        let time = match self.root.playback_mode {
            EffectPlaybackMode::Once | EffectPlaybackMode::LoopRestart => {
                time.clamp(0.0, self.root.duration)
            }
            EffectPlaybackMode::LoopContinuous => time.max(0.0),
        };
        let mut result = vec![ScheduledEffectInstance {
            path: vec![],
            effect: self.root.clone(),
            time,
            seed,
            inherited: root_context.inherited,
            parameter_overrides: vec![],
        }];
        // Parent-before-child order, bounded like reference evaluation even for
        // manually assembled invalid projects. Validated projects are acyclic.
        let mut i = 0;
        while i < result.len() {
            let parent = result[i].clone();
            i += 1;
            if parent.path.len() >= 63 {
                continue;
            }
            let motion = if parent.path.is_empty() {
                root_context.motion.clone()
            } else {
                parent.effect.host_transform_track.clone()
            };
            for source in &parent.effect.effect_clips {
                let Some(clip) = resolve_clip(&parent.path, source) else {
                    continue;
                };
                let Some(child) = self.effect(clip.source.id) else {
                    continue;
                };
                let Some((time, offset)) =
                    clip.map_instance_time(parent.time, &parent.effect, child)
                else {
                    continue;
                };
                let mut path = parent.path.clone();
                path.push(clip.source_clip);
                result.push(ScheduledEffectInstance {
                    path,
                    effect: child.clone(),
                    time,
                    seed: clip.seed.resolve(parent.seed, clip.source_clip),
                    inherited: Arc::new(parent.inherited.for_child(
                        motion.clone(),
                        clip.transform,
                        offset,
                    )),
                    parameter_overrides: clip.parameter_overrides,
                });
            }
        }
        result
    }
}
