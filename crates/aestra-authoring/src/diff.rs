use crate::SemanticTarget;
use aestra_core::{EffectAsset, EffectClip, Emitter, ModuleInstance, RendererInstance};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
    Moved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticChange {
    pub kind: ChangeKind,
    pub target: SemanticTarget,
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectDiff {
    pub changes: Vec<SemanticChange>,
}

impl EffectDiff {
    pub fn between(before: &EffectAsset, after: &EffectAsset) -> Self {
        let mut changes = Vec::new();
        if before.name != after.name {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.name",
                &before.name,
                &after.name,
            );
        }
        if before.duration != after.duration {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.duration",
                before.duration,
                after.duration,
            );
        }
        if before.looping != after.looping {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.looping",
                before.looping,
                after.looping,
            );
        }
        if before.choreography_order != after.choreography_order {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.choreography_order",
                format!("{:?}", before.choreography_order),
                format!("{:?}", after.choreography_order),
            );
        }

        diff_effect_clips(before, after, &mut changes);

        let before_emitters = indexed_emitters(before);
        let after_emitters = indexed_emitters(after);
        for (id, (index, emitter)) in &before_emitters {
            let target = SemanticTarget::Emitter(*id);
            let Some((after_index, after_emitter)) = after_emitters.get(id) else {
                changes.push(SemanticChange {
                    kind: ChangeKind::Removed,
                    target,
                    path: format!("effect.emitters[{index}]"),
                    before: Some(emitter.name.clone()),
                    after: None,
                });
                continue;
            };
            if index != after_index {
                changes.push(SemanticChange {
                    kind: ChangeKind::Moved,
                    target,
                    path: "effect.emitters".into(),
                    before: Some(index.to_string()),
                    after: Some(after_index.to_string()),
                });
            }
            diff_emitter(emitter, after_emitter, &mut changes);
        }
        for (id, (index, emitter)) in &after_emitters {
            if !before_emitters.contains_key(id) {
                changes.push(SemanticChange {
                    kind: ChangeKind::Added,
                    target: SemanticTarget::Emitter(*id),
                    path: format!("effect.emitters[{index}]"),
                    before: None,
                    after: Some(emitter.name.clone()),
                });
            }
        }

        if before.parameters != after.parameters {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.parameters",
                before.parameters.len(),
                after.parameters.len(),
            );
        }
        if before.materials != after.materials {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.materials",
                format!("{:?}", before.materials),
                format!("{:?}", after.materials),
            );
        }
        if before.flipbooks != after.flipbooks {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.flipbooks",
                format!("{:?}", before.flipbooks),
                format!("{:?}", after.flipbooks),
            );
        }
        if before.events != after.events {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.events",
                before.events.len(),
                after.events.len(),
            );
        }
        Self { changes }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

fn diff_effect_clips(before: &EffectAsset, after: &EffectAsset, changes: &mut Vec<SemanticChange>) {
    let before_clips = indexed_effect_clips(&before.effect_clips);
    let after_clips = indexed_effect_clips(&after.effect_clips);
    for (id, (index, clip)) in &before_clips {
        let target = SemanticTarget::EffectClip(*id);
        let Some((after_index, after_clip)) = after_clips.get(id) else {
            changes.push(SemanticChange {
                kind: ChangeKind::Removed,
                target,
                path: format!("effect.effect_clips[{index}]"),
                before: Some(clip.source.to_string()),
                after: None,
            });
            continue;
        };
        if index != after_index {
            changes.push(SemanticChange {
                kind: ChangeKind::Moved,
                target,
                path: "effect.effect_clips".into(),
                before: Some(index.to_string()),
                after: Some(after_index.to_string()),
            });
        }
        if clip != after_clip {
            changes.push(SemanticChange {
                kind: ChangeKind::Modified,
                target,
                path: "effect_clip".into(),
                before: Some(effect_clip_summary(clip)),
                after: Some(effect_clip_summary(after_clip)),
            });
        }
    }
    for (id, (index, clip)) in &after_clips {
        if !before_clips.contains_key(id) {
            changes.push(SemanticChange {
                kind: ChangeKind::Added,
                target: SemanticTarget::EffectClip(*id),
                path: format!("effect.effect_clips[{index}]"),
                before: None,
                after: Some(clip.source.to_string()),
            });
        }
    }
}

fn effect_clip_summary(clip: &EffectClip) -> String {
    format!(
        "source={} start={} offset={} duration={} seed={:?}",
        clip.source, clip.start_time, clip.source_offset, clip.duration, clip.seed
    )
}

fn diff_emitter(before: &Emitter, after: &Emitter, changes: &mut Vec<SemanticChange>) {
    let target = SemanticTarget::Emitter(after.id);
    if before.name != after.name {
        modified(changes, target, "emitter.name", &before.name, &after.name);
    }
    if before.enabled != after.enabled {
        modified(
            changes,
            target,
            "emitter.enabled",
            before.enabled,
            after.enabled,
        );
    }
    if before.transform != after.transform {
        modified(
            changes,
            target,
            "emitter.transform",
            format!("{:?}", before.transform),
            format!("{:?}", after.transform),
        );
    }
    if before.start_time != after.start_time || before.duration != after.duration {
        modified(
            changes,
            target,
            "emitter.timing",
            format!("{}..{}", before.start_time, before.duration),
            format!("{}..{}", after.start_time, after.duration),
        );
    }
    if before.max_particles != after.max_particles {
        modified(
            changes,
            target,
            "emitter.max_particles",
            before.max_particles,
            after.max_particles,
        );
    }
    if before.display_color != after.display_color {
        modified(
            changes,
            target,
            "emitter.display_color",
            format!("{:?}", before.display_color),
            format!("{:?}", after.display_color),
        );
    }
    diff_modules(before, after, changes);
    diff_renderers(before, after, changes);
}

fn diff_modules(before: &Emitter, after: &Emitter, changes: &mut Vec<SemanticChange>) {
    let before_modules = indexed_modules(&before.modules);
    let after_modules = indexed_modules(&after.modules);
    for (id, (index, module)) in &before_modules {
        let target = SemanticTarget::Module(*id);
        let Some((after_index, after_module)) = after_modules.get(id) else {
            changes.push(SemanticChange {
                kind: ChangeKind::Removed,
                target,
                path: format!("emitter.modules[{index}]"),
                before: Some(module.module_type.0.clone()),
                after: None,
            });
            continue;
        };
        if index != after_index {
            changes.push(SemanticChange {
                kind: ChangeKind::Moved,
                target,
                path: "emitter.modules".into(),
                before: Some(index.to_string()),
                after: Some(after_index.to_string()),
            });
        }
        if module != after_module {
            changes.push(SemanticChange {
                kind: ChangeKind::Modified,
                target,
                path: format!("module.{}", module.module_type.0),
                before: Some(format!(
                    "{:?} bindings={:?}",
                    module.parameters, module.bindings
                )),
                after: Some(format!(
                    "{:?} bindings={:?}",
                    after_module.parameters, after_module.bindings
                )),
            });
        }
    }
    for (id, (index, module)) in &after_modules {
        if !before_modules.contains_key(id) {
            changes.push(SemanticChange {
                kind: ChangeKind::Added,
                target: SemanticTarget::Module(*id),
                path: format!("emitter.modules[{index}]"),
                before: None,
                after: Some(module.module_type.0.clone()),
            });
        }
    }
}

fn diff_renderers(before: &Emitter, after: &Emitter, changes: &mut Vec<SemanticChange>) {
    let before_renderers = indexed_renderers(&before.renderers);
    let after_renderers = indexed_renderers(&after.renderers);
    for (id, (index, renderer)) in &before_renderers {
        let target = SemanticTarget::Renderer(*id);
        let Some((after_index, after_renderer)) = after_renderers.get(id) else {
            changes.push(SemanticChange {
                kind: ChangeKind::Removed,
                target,
                path: format!("emitter.renderers[{index}]"),
                before: Some(renderer.renderer_type.0.clone()),
                after: None,
            });
            continue;
        };
        if index != after_index {
            changes.push(SemanticChange {
                kind: ChangeKind::Moved,
                target,
                path: "emitter.renderers".into(),
                before: Some(index.to_string()),
                after: Some(after_index.to_string()),
            });
        }
        if renderer != after_renderer {
            changes.push(SemanticChange {
                kind: ChangeKind::Modified,
                target,
                path: format!("renderer.{}", renderer.renderer_type.0),
                before: Some(format!(
                    "material={} properties={:?}",
                    renderer.material, renderer.properties
                )),
                after: Some(format!(
                    "material={} properties={:?}",
                    after_renderer.material, after_renderer.properties
                )),
            });
        }
    }
    for (id, (index, renderer)) in &after_renderers {
        if !before_renderers.contains_key(id) {
            changes.push(SemanticChange {
                kind: ChangeKind::Added,
                target: SemanticTarget::Renderer(*id),
                path: format!("emitter.renderers[{index}]"),
                before: None,
                after: Some(renderer.renderer_type.0.clone()),
            });
        }
    }
}

fn indexed_emitters(effect: &EffectAsset) -> BTreeMap<aestra_core::EmitterId, (usize, &Emitter)> {
    effect
        .emitters
        .iter()
        .enumerate()
        .map(|(index, emitter)| (emitter.id, (index, emitter)))
        .collect()
}

fn indexed_effect_clips(
    clips: &[EffectClip],
) -> BTreeMap<aestra_core::EffectClipId, (usize, &EffectClip)> {
    clips
        .iter()
        .enumerate()
        .map(|(index, clip)| (clip.id, (index, clip)))
        .collect()
}

fn indexed_modules(
    modules: &[ModuleInstance],
) -> BTreeMap<aestra_core::ModuleId, (usize, &ModuleInstance)> {
    modules
        .iter()
        .enumerate()
        .map(|(index, module)| (module.id, (index, module)))
        .collect()
}

fn indexed_renderers(
    renderers: &[RendererInstance],
) -> BTreeMap<aestra_core::RendererId, (usize, &RendererInstance)> {
    renderers
        .iter()
        .enumerate()
        .map(|(index, renderer)| (renderer.id, (index, renderer)))
        .collect()
}

fn modified(
    changes: &mut Vec<SemanticChange>,
    target: SemanticTarget,
    path: impl Into<String>,
    before: impl ToString,
    after: impl ToString,
) {
    changes.push(SemanticChange {
        kind: ChangeKind::Modified,
        target,
        path: path.into(),
        before: Some(before.to_string()),
        after: Some(after.to_string()),
    });
}
