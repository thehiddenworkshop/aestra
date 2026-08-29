use crate::EffectCommand;
use aestra_core::{
    CurveId, EffectAsset, EffectClipId, EffectId, EmitterId, EventId, GradientId, ModuleId,
    ModuleParameters, ParameterId, RendererId,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SemanticTarget {
    Effect(EffectId),
    EffectClip(EffectClipId),
    Parameter(ParameterId),
    Emitter(EmitterId),
    Module(ModuleId),
    Renderer(RendererId),
    Curve(CurveId),
    Gradient(GradientId),
    Event(EventId),
}

impl fmt::Display for SemanticTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Effect(id) => write!(formatter, "effect {id}"),
            Self::EffectClip(id) => write!(formatter, "effect clip {id}"),
            Self::Parameter(id) => write!(formatter, "parameter {id}"),
            Self::Emitter(id) => write!(formatter, "emitter {id}"),
            Self::Module(id) => write!(formatter, "module {id}"),
            Self::Renderer(id) => write!(formatter, "renderer {id}"),
            Self::Curve(id) => write!(formatter, "curve {id}"),
            Self::Gradient(id) => write!(formatter, "gradient {id}"),
            Self::Event(id) => write!(formatter, "event {id}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub primary: SemanticTarget,
}

impl Selection {
    pub fn for_effect(effect: &EffectAsset) -> Self {
        let primary = effect
            .emitters
            .first()
            .map_or(SemanticTarget::Effect(effect.id), |emitter| {
                SemanticTarget::Emitter(emitter.id)
            });
        Self { primary }
    }

    pub fn select_emitter(&mut self, id: EmitterId) {
        self.primary = SemanticTarget::Emitter(id);
    }

    pub fn select_effect_clip(&mut self, id: EffectClipId) {
        self.primary = SemanticTarget::EffectClip(id);
    }

    pub fn effect_clip(&self) -> Option<EffectClipId> {
        match self.primary {
            SemanticTarget::EffectClip(id) => Some(id),
            _ => None,
        }
    }

    pub fn emitter(&self, effect: &EffectAsset) -> Option<EmitterId> {
        match self.primary {
            SemanticTarget::Emitter(id) => Some(id),
            SemanticTarget::Module(id) => effect
                .emitters
                .iter()
                .find(|emitter| emitter.modules.iter().any(|module| module.id == id))
                .map(|emitter| emitter.id),
            SemanticTarget::Renderer(id) => effect
                .emitters
                .iter()
                .find(|emitter| emitter.renderers.iter().any(|renderer| renderer.id == id))
                .map(|emitter| emitter.id),
            SemanticTarget::Curve(id) => effect
                .emitters
                .iter()
                .find(|emitter| emitter.size_curve().id == id || emitter.opacity_curve().id == id)
                .map(|emitter| emitter.id),
            SemanticTarget::Gradient(id) => effect
                .emitters
                .iter()
                .find(|emitter| emitter.color_gradient().id == id)
                .map(|emitter| emitter.id),
            _ => None,
        }
    }

    pub fn repair(&mut self, effect: &EffectAsset) {
        if !target_exists(self.primary, effect) {
            *self = Self::for_effect(effect);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockState {
    locked: BTreeSet<SemanticTarget>,
}

impl LockState {
    pub fn lock(&mut self, target: SemanticTarget) {
        self.locked.insert(target);
    }

    pub fn unlock(&mut self, target: SemanticTarget) {
        self.locked.remove(&target);
    }

    pub fn is_locked(&self, target: SemanticTarget) -> bool {
        self.locked.contains(&target)
    }

    pub fn iter(&self) -> impl Iterator<Item = SemanticTarget> + '_ {
        self.locked.iter().copied()
    }

    pub(crate) fn blocking_target(
        &self,
        command: &EffectCommand,
        effect: &EffectAsset,
    ) -> Option<SemanticTarget> {
        let effect_target = SemanticTarget::Effect(effect.id);
        if self.is_locked(effect_target) {
            return Some(effect_target);
        }

        let (emitter_id, direct) = command_targets(command);
        if let Some(target) = direct
            && self.is_locked(target)
        {
            return Some(target);
        }
        if let Some(emitter_id) = emitter_id {
            let target = SemanticTarget::Emitter(emitter_id);
            if self.is_locked(target) {
                return Some(target);
            }
        }

        if let EffectCommand::SetModuleParameter {
            emitter,
            module,
            parameter,
            ..
        } = command
            && let Some(module) = effect
                .emitters
                .iter()
                .find(|item| item.id == *emitter)
                .and_then(|item| item.modules.iter().find(|item| item.id == *module))
            && let ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } = &module.parameters
        {
            let nested = match parameter.as_str() {
                "size" => Some(SemanticTarget::Curve(size.id)),
                "opacity" => Some(SemanticTarget::Curve(opacity.id)),
                "color" => Some(SemanticTarget::Gradient(color.id)),
                _ => None,
            };
            if let Some(target) = nested
                && self.is_locked(target)
            {
                return Some(target);
            }
        }

        let nested_edit = match command {
            EffectCommand::AddCurveKey {
                emitter,
                module,
                parameter,
                ..
            }
            | EffectCommand::RemoveCurveKey {
                emitter,
                module,
                parameter,
                ..
            }
            | EffectCommand::SetCurveKey {
                emitter,
                module,
                parameter,
                ..
            }
            | EffectCommand::AddGradientKey {
                emitter,
                module,
                parameter,
                ..
            }
            | EffectCommand::RemoveGradientKey {
                emitter,
                module,
                parameter,
                ..
            }
            | EffectCommand::SetGradientKey {
                emitter,
                module,
                parameter,
                ..
            } => Some((*emitter, *module, parameter.as_str())),
            _ => None,
        };
        if let Some((emitter, module, parameter)) = nested_edit
            && let Some(module) = effect
                .emitters
                .iter()
                .find(|item| item.id == emitter)
                .and_then(|item| item.modules.iter().find(|item| item.id == module))
            && let ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } = &module.parameters
        {
            let target = match parameter {
                "size" => Some(SemanticTarget::Curve(size.id)),
                "opacity" => Some(SemanticTarget::Curve(opacity.id)),
                "color" => Some(SemanticTarget::Gradient(color.id)),
                _ => None,
            };
            if target.is_some_and(|target| self.is_locked(target)) {
                return target;
            }
        }

        if let EffectCommand::RemoveModule { emitter, module } = command
            && let Some(module) = effect
                .emitters
                .iter()
                .find(|item| item.id == *emitter)
                .and_then(|item| item.modules.iter().find(|item| item.id == *module))
            && let ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } = &module.parameters
        {
            for target in [
                SemanticTarget::Curve(size.id),
                SemanticTarget::Curve(opacity.id),
                SemanticTarget::Gradient(color.id),
            ] {
                if self.is_locked(target) {
                    return Some(target);
                }
            }
        }

        if let EffectCommand::RemoveEmitter { id } = command
            && let Some(emitter) = effect.emitters.iter().find(|emitter| emitter.id == *id)
        {
            for target in self.iter() {
                if target == SemanticTarget::Emitter(*id)
                    || matches!(target, SemanticTarget::Module(module) if emitter.modules.iter().any(|item| item.id == module))
                    || matches!(target, SemanticTarget::Renderer(renderer) if emitter.renderers.iter().any(|item| item.id == renderer))
                    || matches!(target, SemanticTarget::Curve(curve) if emitter.size_curve().id == curve || emitter.opacity_curve().id == curve)
                    || matches!(target, SemanticTarget::Gradient(gradient) if emitter.color_gradient().id == gradient)
                    || matches!(target, SemanticTarget::Event(event) if effect.events.iter().any(|item| item.id == event && (item.source == *id || item.target == *id)))
                {
                    return Some(target);
                }
            }
        }
        None
    }
}

fn command_targets(command: &EffectCommand) -> (Option<EmitterId>, Option<SemanticTarget>) {
    match command {
        EffectCommand::SetEffectName { .. }
        | EffectCommand::SetEffectDuration { .. }
        | EffectCommand::SetEffectLooping { .. }
        | EffectCommand::SetChoreographyOrder { .. }
        | EffectCommand::AddEffectClip { .. }
        | EffectCommand::AddEmitter { .. }
        | EffectCommand::AddParameter { .. }
        | EffectCommand::AddMaterial { .. }
        | EffectCommand::RemoveMaterial { .. }
        | EffectCommand::SetMaterial { .. }
        | EffectCommand::AddFlipbook { .. }
        | EffectCommand::RemoveFlipbook { .. }
        | EffectCommand::SetFlipbook { .. }
        | EffectCommand::AddEvent { .. } => (None, None),
        EffectCommand::RemoveEffectClip { id }
        | EffectCommand::MoveEffectClip { id, .. }
        | EffectCommand::SetEffectClipTiming { id, .. }
        | EffectCommand::SetEffectClipSeed { id, .. }
        | EffectCommand::SetEffectClipSource { id, .. }
        | EffectCommand::SetEffectClipTransform { id, .. } => {
            (None, Some(SemanticTarget::EffectClip(*id)))
        }
        EffectCommand::RemoveParameter { id } => (None, Some(SemanticTarget::Parameter(*id))),
        EffectCommand::RemoveEmitter { id }
        | EffectCommand::MoveEmitter { id, .. }
        | EffectCommand::SetEmitterName { id, .. }
        | EffectCommand::SetEmitterEnabled { id, .. }
        | EffectCommand::SetEmitterDisplayColor { id, .. }
        | EffectCommand::SetEmitterTransform { id, .. }
        | EffectCommand::SetEmitterTiming { id, .. }
        | EffectCommand::SetEmitterCapacity { id, .. } => {
            (Some(*id), Some(SemanticTarget::Emitter(*id)))
        }
        EffectCommand::AddModule { emitter, .. } | EffectCommand::AddRenderer { emitter, .. } => {
            (Some(*emitter), None)
        }
        EffectCommand::RemoveModule {
            emitter, module, ..
        }
        | EffectCommand::MoveModule {
            emitter, module, ..
        }
        | EffectCommand::SetModuleEnabled {
            emitter, module, ..
        }
        | EffectCommand::SetModuleParameter {
            emitter, module, ..
        }
        | EffectCommand::RemoveModuleParameter {
            emitter, module, ..
        }
        | EffectCommand::BindModuleParameter {
            emitter, module, ..
        }
        | EffectCommand::UnbindModuleParameter {
            emitter, module, ..
        }
        | EffectCommand::AddCurveKey {
            emitter, module, ..
        }
        | EffectCommand::RemoveCurveKey {
            emitter, module, ..
        }
        | EffectCommand::SetCurveKey {
            emitter, module, ..
        }
        | EffectCommand::AddGradientKey {
            emitter, module, ..
        }
        | EffectCommand::RemoveGradientKey {
            emitter, module, ..
        }
        | EffectCommand::SetGradientKey {
            emitter, module, ..
        } => (Some(*emitter), Some(SemanticTarget::Module(*module))),
        EffectCommand::RemoveRenderer {
            emitter, renderer, ..
        }
        | EffectCommand::MoveRenderer {
            emitter, renderer, ..
        }
        | EffectCommand::SetRendererEnabled {
            emitter, renderer, ..
        }
        | EffectCommand::SetRendererMaterial {
            emitter, renderer, ..
        }
        | EffectCommand::SetRendererProperties {
            emitter, renderer, ..
        } => (Some(*emitter), Some(SemanticTarget::Renderer(*renderer))),
        EffectCommand::RemoveEvent { id } => (None, Some(SemanticTarget::Event(*id))),
    }
}

fn target_exists(target: SemanticTarget, effect: &EffectAsset) -> bool {
    match target {
        SemanticTarget::Effect(id) => effect.id == id,
        SemanticTarget::EffectClip(id) => effect.effect_clips.iter().any(|clip| clip.id == id),
        SemanticTarget::Parameter(id) => effect.parameters.iter().any(|item| item.id == id),
        SemanticTarget::Emitter(id) => effect.emitters.iter().any(|item| item.id == id),
        SemanticTarget::Module(id) => effect
            .emitters
            .iter()
            .any(|emitter| emitter.modules.iter().any(|item| item.id == id)),
        SemanticTarget::Renderer(id) => effect
            .emitters
            .iter()
            .any(|emitter| emitter.renderers.iter().any(|item| item.id == id)),
        SemanticTarget::Curve(id) => effect
            .emitters
            .iter()
            .any(|emitter| emitter.size_curve().id == id || emitter.opacity_curve().id == id),
        SemanticTarget::Gradient(id) => effect
            .emitters
            .iter()
            .any(|emitter| emitter.color_gradient().id == id),
        SemanticTarget::Event(id) => effect.events.iter().any(|event| event.id == id),
    }
}
