use crate::SemanticTarget;
use aestra_core::{
    ChoreographyEvent, EffectAsset, EffectClip, EffectMarker, Emitter, ModuleInstance,
    RendererInstance,
};
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
        if before.particle_budgets != after.particle_budgets {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.particle_budgets",
                format!("{:?}", before.particle_budgets),
                format!("{:?}", after.particle_budgets),
            );
        }
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
        if before.playback_mode != after.playback_mode {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.playback_mode",
                before.playback_mode,
                after.playback_mode,
            );
        }
        if before.host_transform_track != after.host_transform_track {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.host_transform_track",
                format!("{:?}", before.host_transform_track),
                format!("{:?}", after.host_transform_track),
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
        diff_markers(before, after, &mut changes);
        diff_choreography_events(before, after, &mut changes);
        for (index, binding) in before.point_lights.iter().enumerate() {
            let current = after.point_lights.iter().find(|item| item.id == binding.id);
            if current != Some(binding) {
                changes.push(SemanticChange {
                    kind: if current.is_some() {
                        ChangeKind::Modified
                    } else {
                        ChangeKind::Removed
                    },
                    target: SemanticTarget::PointLight(binding.id),
                    path: format!("effect.point_lights[{index}]"),
                    before: Some(format!("{binding:?}")),
                    after: current.map(|item| format!("{item:?}")),
                });
            }
        }
        for (index, binding) in after.point_lights.iter().enumerate() {
            if !before.point_lights.iter().any(|item| item.id == binding.id) {
                changes.push(SemanticChange {
                    kind: ChangeKind::Added,
                    target: SemanticTarget::PointLight(binding.id),
                    path: format!("effect.point_lights[{index}]"),
                    before: None,
                    after: Some(format!("{binding:?}")),
                });
            }
        }

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
        if before.bindings != after.bindings {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.bindings",
                before.bindings.len(),
                after.bindings.len(),
            );
        }
        // Declared events (event system E1): which ones, by name.
        for (path, before_events, after_events) in [
            (
                "effect.event_inputs",
                &before.event_inputs,
                &after.event_inputs,
            ),
            (
                "effect.event_outputs",
                &before.event_outputs,
                &after.event_outputs,
            ),
        ] {
            if before_events != after_events {
                let names = |events: &[aestra_core::EventDefinition]| {
                    events
                        .iter()
                        .map(|event| event.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                modified(
                    &mut changes,
                    SemanticTarget::Effect(after.id),
                    path,
                    names(before_events),
                    names(after_events),
                );
            }
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
        if before.material_instances != after.material_instances {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.material_instances",
                format!("{:?}", before.material_instances),
                format!("{:?}", after.material_instances),
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
        // The effect's own simulation stages (fluid F2).
        if before.simulation_stages != after.simulation_stages {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.simulation_stages",
                format!("{:?}", before.simulation_stages),
                format!("{:?}", after.simulation_stages),
            );
        }
        // Links added, removed or reordered.
        let ids = |effect: &EffectAsset| {
            effect
                .events
                .iter()
                .map(|event| event.id)
                .collect::<Vec<_>>()
        };
        if ids(before) != ids(after) {
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.events",
                before.events.len(),
                after.events.len(),
            );
        }
        // A link edited in place (event system E0): its trigger, target, count or inheritance.
        for (index, event) in after.events.iter().enumerate() {
            if let Some(previous) = before
                .events
                .iter()
                .find(|previous| previous.id == event.id)
                && previous != event
            {
                let summary = |link: &aestra_core::EventLink| {
                    format!(
                        "{:?} -> {} x{} inherit {}",
                        link.trigger, link.target, link.count, link.inherit_velocity
                    )
                };
                modified(
                    &mut changes,
                    SemanticTarget::Event(event.id),
                    format!("effect.events[{index}]"),
                    summary(previous),
                    summary(event),
                );
            }
        }
        // Input routes (event system E3) added, removed, reordered or edited.
        if before.input_spawns != after.input_spawns {
            let summary = |effect: &EffectAsset| {
                effect
                    .input_spawns
                    .iter()
                    .map(|route| format!("{} -> {} x{}", route.input, route.target, route.count))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.input_spawns",
                summary(before),
                summary(after),
            );
        }
        if before.particle_outputs != after.particle_outputs {
            let summary = |effect: &EffectAsset| {
                effect
                    .particle_outputs
                    .iter()
                    .map(|route| {
                        format!(
                            "{} {:?} -> {} {:?}",
                            route.source, route.trigger, route.output, route.aggregation
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            modified(
                &mut changes,
                SemanticTarget::Effect(after.id),
                "effect.particle_outputs",
                summary(before),
                summary(after),
            );
        }
        Self { changes }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

fn diff_choreography_events(
    before: &EffectAsset,
    after: &EffectAsset,
    changes: &mut Vec<SemanticChange>,
) {
    let before_events = indexed_choreography_events(&before.choreography_events);
    let after_events = indexed_choreography_events(&after.choreography_events);
    for (id, (index, event)) in &before_events {
        let target = SemanticTarget::ChoreographyEvent(*id);
        let Some((after_index, after_event)) = after_events.get(id) else {
            changes.push(SemanticChange {
                kind: ChangeKind::Removed,
                target,
                path: format!("effect.choreography_events[{index}]"),
                before: Some(event.name.clone()),
                after: None,
            });
            continue;
        };
        if index != after_index {
            changes.push(SemanticChange {
                kind: ChangeKind::Moved,
                target,
                path: "effect.choreography_events".into(),
                before: Some(index.to_string()),
                after: Some(after_index.to_string()),
            });
        }
        if event != after_event {
            changes.push(SemanticChange {
                kind: ChangeKind::Modified,
                target,
                path: "choreography_event".into(),
                before: Some(format!("{} @ {}", event.name, event.time)),
                after: Some(format!("{} @ {}", after_event.name, after_event.time)),
            });
        }
    }
    for (id, (index, event)) in &after_events {
        if !before_events.contains_key(id) {
            changes.push(SemanticChange {
                kind: ChangeKind::Added,
                target: SemanticTarget::ChoreographyEvent(*id),
                path: format!("effect.choreography_events[{index}]"),
                before: None,
                after: Some(event.name.clone()),
            });
        }
    }
}

fn diff_markers(before: &EffectAsset, after: &EffectAsset, changes: &mut Vec<SemanticChange>) {
    let before_markers = indexed_markers(&before.markers);
    let after_markers = indexed_markers(&after.markers);
    for (id, (index, marker)) in &before_markers {
        let target = SemanticTarget::Marker(*id);
        let Some((after_index, after_marker)) = after_markers.get(id) else {
            changes.push(SemanticChange {
                kind: ChangeKind::Removed,
                target,
                path: format!("effect.markers[{index}]"),
                before: Some(marker.name.clone()),
                after: None,
            });
            continue;
        };
        if index != after_index {
            changes.push(SemanticChange {
                kind: ChangeKind::Moved,
                target,
                path: "effect.markers".into(),
                before: Some(index.to_string()),
                after: Some(after_index.to_string()),
            });
        }
        if marker != after_marker {
            changes.push(SemanticChange {
                kind: ChangeKind::Modified,
                target,
                path: "marker".into(),
                before: Some(format!("{} @ {}", marker.name, marker.time)),
                after: Some(format!("{} @ {}", after_marker.name, after_marker.time)),
            });
        }
    }
    for (id, (index, marker)) in &after_markers {
        if !before_markers.contains_key(id) {
            changes.push(SemanticChange {
                kind: ChangeKind::Added,
                target: SemanticTarget::Marker(*id),
                path: format!("effect.markers[{index}]"),
                before: None,
                after: Some(marker.name.clone()),
            });
        }
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
    if before.start_time != after.start_time
        || before.start_reference != after.start_reference
        || before.duration != after.duration
        || before.regions != after.regions
    {
        modified(
            changes,
            target,
            "emitter.timing",
            format!(
                "{} ({:?})..{} regions={:?}",
                before.start_time, before.start_reference, before.duration, before.regions
            ),
            format!(
                "{} ({:?})..{} regions={:?}",
                after.start_time, after.start_reference, after.duration, after.regions
            ),
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
    if before.scene_outputs != after.scene_outputs {
        modified(
            changes,
            target,
            "emitter.scene_outputs",
            format!("{:?}", before.scene_outputs),
            format!("{:?}", after.scene_outputs),
        );
    }
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
                    "{:?} sources={:?} source_values={:?} bindings={:?} host={:?}",
                    module.parameters,
                    module.property_sources,
                    module.property_source_values,
                    module.bindings,
                    module.host_bindings
                )),
                after: Some(format!(
                    "{:?} sources={:?} source_values={:?} bindings={:?} host={:?}",
                    after_module.parameters,
                    after_module.property_sources,
                    after_module.property_source_values,
                    after_module.bindings,
                    after_module.host_bindings
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

fn indexed_markers(
    markers: &[EffectMarker],
) -> BTreeMap<aestra_core::MarkerId, (usize, &EffectMarker)> {
    markers
        .iter()
        .enumerate()
        .map(|(index, marker)| (marker.id, (index, marker)))
        .collect()
}

fn indexed_choreography_events(
    events: &[ChoreographyEvent],
) -> BTreeMap<aestra_core::ChoreographyEventId, (usize, &ChoreographyEvent)> {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| (event.id, (index, event)))
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
