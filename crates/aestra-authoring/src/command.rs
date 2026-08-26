use aestra_core::{
    BlendMode, EffectParameter, Emitter, EmitterId, EventId, EventLink, ModuleId, ModuleInstance,
    RendererId, RendererInstance, RendererProperties, Value,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EffectCommand {
    SetEffectName {
        name: String,
    },
    SetEffectDuration {
        duration: f32,
    },
    SetEffectLooping {
        looping: bool,
    },
    AddParameter {
        parameter: EffectParameter,
        index: usize,
    },
    RemoveParameter {
        id: aestra_core::ParameterId,
    },
    AddEmitter {
        emitter: Emitter,
        index: usize,
    },
    RemoveEmitter {
        id: EmitterId,
    },
    MoveEmitter {
        id: EmitterId,
        index: usize,
    },
    SetEmitterName {
        id: EmitterId,
        name: String,
    },
    SetEmitterEnabled {
        id: EmitterId,
        enabled: bool,
    },
    SetEmitterTiming {
        id: EmitterId,
        start_time: f32,
        duration: f32,
    },
    SetEmitterCapacity {
        id: EmitterId,
        max_particles: u32,
    },
    AddModule {
        emitter: EmitterId,
        module: ModuleInstance,
        index: usize,
    },
    RemoveModule {
        emitter: EmitterId,
        module: ModuleId,
    },
    MoveModule {
        emitter: EmitterId,
        module: ModuleId,
        index: usize,
    },
    SetModuleEnabled {
        emitter: EmitterId,
        module: ModuleId,
        enabled: bool,
    },
    SetModuleParameter {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
        value: Value,
    },
    RemoveModuleParameter {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
    },
    BindModuleParameter {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
        source: aestra_core::ParameterId,
    },
    UnbindModuleParameter {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
    },
    AddRenderer {
        emitter: EmitterId,
        renderer: RendererInstance,
        index: usize,
    },
    RemoveRenderer {
        emitter: EmitterId,
        renderer: RendererId,
    },
    MoveRenderer {
        emitter: EmitterId,
        renderer: RendererId,
        index: usize,
    },
    SetRendererEnabled {
        emitter: EmitterId,
        renderer: RendererId,
        enabled: bool,
    },
    SetRendererBlend {
        emitter: EmitterId,
        renderer: RendererId,
        blend: BlendMode,
    },
    SetRendererProperties {
        emitter: EmitterId,
        renderer: RendererId,
        properties: RendererProperties,
    },
    AddEvent {
        event: EventLink,
        index: usize,
    },
    RemoveEvent {
        id: EventId,
    },
}

impl EffectCommand {
    pub fn duplicate_emitter(effect: &aestra_core::EffectAsset, id: EmitterId) -> Option<Self> {
        let index = effect
            .emitters
            .iter()
            .position(|emitter| emitter.id == id)?;
        let mut emitter = effect.emitters[index].clone();
        emitter.regenerate_ids();
        emitter.name = format!("{} Copy", emitter.name);
        Some(Self::AddEmitter {
            emitter,
            index: index + 1,
        })
    }

    pub fn duplicate_module(
        effect: &aestra_core::EffectAsset,
        emitter: EmitterId,
        id: ModuleId,
    ) -> Option<Self> {
        let emitter = effect.emitters.iter().find(|item| item.id == emitter)?;
        let index = emitter.modules.iter().position(|module| module.id == id)?;
        let mut module = emitter.modules[index].clone();
        module.regenerate_ids();
        Some(Self::AddModule {
            emitter: emitter.id,
            module,
            index: index + 1,
        })
    }

    pub fn duplicate_renderer(
        effect: &aestra_core::EffectAsset,
        emitter: EmitterId,
        id: RendererId,
    ) -> Option<Self> {
        let emitter = effect.emitters.iter().find(|item| item.id == emitter)?;
        let index = emitter
            .renderers
            .iter()
            .position(|renderer| renderer.id == id)?;
        let mut renderer = emitter.renderers[index].clone();
        renderer.id = RendererId::new();
        Some(Self::AddRenderer {
            emitter: emitter.id,
            renderer,
            index: index + 1,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectTransaction {
    pub label: String,
    pub commands: Vec<EffectCommand>,
}

impl EffectTransaction {
    pub fn new(label: impl Into<String>, commands: Vec<EffectCommand>) -> Self {
        Self {
            label: label.into(),
            commands,
        }
    }

    pub fn single(label: impl Into<String>, command: EffectCommand) -> Self {
        Self::new(label, vec![command])
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}
