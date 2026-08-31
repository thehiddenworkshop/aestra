use aestra_core::{
    AssetDefinition, AssetId, ChoreographyEvent, ChoreographyEventId, ChoreographyEventPayload,
    ChoreographyTrackId, ColorKey, CurveKey, EffectAssetRef, EffectClip, EffectClipId,
    EffectClipSeed, EffectMarker, EffectParameter, Emitter, EmitterId, EmitterTransform, EventId,
    EventLink, FlipbookDefinition, MarkerId, MarkerTimeReference, MaterialDefinition, MaterialId,
    ModuleId, ModuleInstance, ParameterId, PropertySource, RendererId, RendererInstance,
    RendererProperties, Value,
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
    SetChoreographyOrder {
        order: Vec<ChoreographyTrackId>,
    },
    AddMarker {
        marker: EffectMarker,
        index: usize,
    },
    RemoveMarker {
        id: MarkerId,
    },
    SetMarkerName {
        id: MarkerId,
        name: String,
    },
    SetMarkerTime {
        id: MarkerId,
        time: f32,
    },
    AddChoreographyEvent {
        event: ChoreographyEvent,
        index: usize,
    },
    RemoveChoreographyEvent {
        id: ChoreographyEventId,
    },
    SetChoreographyEventName {
        id: ChoreographyEventId,
        name: String,
    },
    SetChoreographyEventTime {
        id: ChoreographyEventId,
        time: f32,
    },
    SetChoreographyEventTimeReference {
        id: ChoreographyEventId,
        reference: Option<MarkerTimeReference>,
    },
    SetChoreographyEventPayload {
        id: ChoreographyEventId,
        payload: ChoreographyEventPayload,
    },
    AddEffectClip {
        clip: EffectClip,
        index: usize,
    },
    RemoveEffectClip {
        id: EffectClipId,
    },
    MoveEffectClip {
        id: EffectClipId,
        index: usize,
    },
    SetEffectClipTiming {
        id: EffectClipId,
        start_time: f32,
        source_offset: f32,
        duration: f32,
    },
    SetEffectClipStartReference {
        id: EffectClipId,
        reference: Option<MarkerTimeReference>,
    },
    SetEffectClipSeed {
        id: EffectClipId,
        seed: EffectClipSeed,
    },
    SetEffectClipSource {
        id: EffectClipId,
        source: EffectAssetRef,
    },
    SetEffectClipTransform {
        id: EffectClipId,
        transform: EmitterTransform,
    },
    SetEffectClipParameterOverride {
        id: EffectClipId,
        parameter: ParameterId,
        value: Value,
    },
    RemoveEffectClipParameterOverride {
        id: EffectClipId,
        parameter: ParameterId,
    },
    AddAsset {
        asset: AssetDefinition,
        index: usize,
    },
    RemoveAsset {
        id: AssetId,
    },
    AddParameter {
        parameter: EffectParameter,
        index: usize,
    },
    RemoveParameter {
        id: aestra_core::ParameterId,
    },
    SetParameter {
        id: aestra_core::ParameterId,
        parameter: EffectParameter,
    },
    AddMaterial {
        material: MaterialDefinition,
        index: usize,
    },
    RemoveMaterial {
        id: MaterialId,
    },
    SetMaterial {
        id: MaterialId,
        material: MaterialDefinition,
    },
    AddFlipbook {
        flipbook: FlipbookDefinition,
        index: usize,
    },
    RemoveFlipbook {
        id: aestra_core::AssetId,
    },
    SetFlipbook {
        id: aestra_core::AssetId,
        flipbook: FlipbookDefinition,
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
    SetEmitterDisplayColor {
        id: EmitterId,
        color: Option<[f32; 4]>,
    },
    SetEmitterTransform {
        id: EmitterId,
        transform: EmitterTransform,
    },
    SetEmitterTiming {
        id: EmitterId,
        start_time: f32,
        duration: f32,
    },
    SetEmitterStartReference {
        id: EmitterId,
        reference: Option<MarkerTimeReference>,
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
    SetModulePropertySource {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
        source: PropertySource,
    },
    RemoveModulePropertySource {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
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
    AddCurveKey {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
        key: CurveKey,
        index: usize,
    },
    RemoveCurveKey {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
        index: usize,
    },
    SetCurveKey {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
        index: usize,
        key: CurveKey,
    },
    AddGradientKey {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
        key: ColorKey,
        index: usize,
    },
    RemoveGradientKey {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
        index: usize,
    },
    SetGradientKey {
        emitter: EmitterId,
        module: ModuleId,
        parameter: String,
        index: usize,
        key: ColorKey,
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
    SetRendererMaterial {
        emitter: EmitterId,
        renderer: RendererId,
        material: MaterialId,
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
