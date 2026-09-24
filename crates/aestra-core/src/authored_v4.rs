//! The authored **format v4** document shape and its lossless mapping to/from the flat in-memory model
//! (extensible-stages redesign M3).
//!
//! v4 makes stages first-class in the *authored file*: an emitter's modules live inside named lifecycle
//! containers (`emitter_spawn` / `emitter_update` / `particle_spawn` / `particle_update`) and explicit
//! `simulation_stages`, rather than a flat `modules` list where each module carries a `stage` field; the
//! emitter's `simulation_domain` becomes a namespaced `DomainTypeId`; and the effect reserves its own
//! `lifecycle` (effect_spawn / effect_update) slots. Per the chosen approach the **in-memory model stays
//! flat** — [`AuthoredV4Document`] is a serde DTO that nests on save and flattens on load, reconstructing
//! each module's `stage` from the container it sits in. Because the format can be re-cut with a one-shot
//! converter while Aestra is pre-release, the renderer-payload generalization (§16) is left for M8's
//! renderer work rather than crammed in here.
//!
//! Conversion canonicalizes module order by stage (the order v4 defines); real authored assets are
//! already authored in that order, so a v3→v4→flat round trip is an identity for them.

use crate::{
    AssetDefinition, AssetId, ChoreographyEvent, ChoreographyTrackId, DomainTypeId, EffectClip,
    EffectId, EffectMarker, EffectParameter, EffectPlaybackMode, Emitter, EmitterId, EmitterRegion,
    EmitterTransform, EventLink, FlipbookDefinition, HostTransformTrack, MarkerTimeReference,
    MaterialDefinition, ModuleId, ModuleInstance, ModuleParameters, ModuleTypeId, ParameterId,
    PropertySource, PropertySourceValue, RendererInstance, SimulationDomain, StageId, StageKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The registered stage-type identities for the built-in lifecycle roles (extensible-stages M3, §31.1).
pub const AESTRA_STAGE_EFFECT_SPAWN: &str = "aestra.stage.effect_spawn";
pub const AESTRA_STAGE_EFFECT_UPDATE: &str = "aestra.stage.effect_update";
pub const AESTRA_STAGE_EMITTER_SPAWN: &str = "aestra.stage.emitter_spawn";
pub const AESTRA_STAGE_EMITTER_UPDATE: &str = "aestra.stage.emitter_update";
pub const AESTRA_STAGE_PARTICLE_SPAWN: &str = "aestra.stage.particle_spawn";
pub const AESTRA_STAGE_PARTICLE_UPDATE: &str = "aestra.stage.particle_update";

/// The registered domain identities for the built-in `SimulationDomain`s (extensible-stages M3, §10).
pub const AESTRA_DOMAIN_PARTICLES: &str = "aestra.domain.particles";
pub const AESTRA_DOMAIN_STRIP: &str = "aestra.domain.strip";

/// Why a flat asset cannot be expressed in the v4 document shape (or vice versa).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum V4ConversionError {
    /// An emitter module declares an effect-level stage (`EffectSpawn`/`EffectUpdate`). Effect-level
    /// modules are not representable in the current flat per-emitter model; no built-in produces them.
    EffectLevelModuleOnEmitter {
        emitter: String,
        module: ModuleTypeId,
    },
}

impl std::fmt::Display for V4ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EffectLevelModuleOnEmitter { emitter, module } => write!(
                f,
                "emitter '{emitter}' carries an effect-level module '{}' which the flat v4 mapping does \
                 not yet support",
                module.0
            ),
        }
    }
}

impl std::error::Error for V4ConversionError {}

/// A module inside a v4 stage container: [`ModuleInstance`] without its `stage` field — the stage is
/// implied by the container the module sits in (structural containment, §31.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V4Module {
    pub id: ModuleId,
    pub module_type: ModuleTypeId,
    pub enabled: bool,
    pub parameters: ModuleParameters,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub property_sources: BTreeMap<String, PropertySource>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub property_source_values: BTreeMap<String, Vec<PropertySourceValue>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, ParameterId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl V4Module {
    fn from_module(module: &ModuleInstance) -> Self {
        Self {
            id: module.id,
            module_type: module.module_type.clone(),
            enabled: module.enabled,
            parameters: module.parameters.clone(),
            property_sources: module.property_sources.clone(),
            property_source_values: module.property_source_values.clone(),
            bindings: module.bindings.clone(),
            label: module.label.clone(),
        }
    }

    fn into_module(self, stage: StageKind) -> ModuleInstance {
        ModuleInstance {
            id: self.id,
            module_type: self.module_type,
            stage,
            enabled: self.enabled,
            parameters: self.parameters,
            property_sources: self.property_sources,
            property_source_values: self.property_source_values,
            bindings: self.bindings,
            label: self.label,
        }
    }
}

/// A container holding the modules of one lifecycle stage, in order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct V4StageModules {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<V4Module>,
}

/// The effect's own lifecycle slots (§4.2). Reserved in v4; currently always empty (the flat model has
/// no effect-level module storage yet — that arrives with the in-memory containment refactor).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct V4EffectLifecycle {
    #[serde(default, skip_serializing_if = "V4StageModules::is_empty")]
    pub effect_spawn: V4StageModules,
    #[serde(default, skip_serializing_if = "V4StageModules::is_empty")]
    pub effect_update: V4StageModules,
}

/// An emitter's four ordered lifecycle stages.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct V4EmitterLifecycle {
    #[serde(default, skip_serializing_if = "V4StageModules::is_empty")]
    pub emitter_spawn: V4StageModules,
    #[serde(default, skip_serializing_if = "V4StageModules::is_empty")]
    pub emitter_update: V4StageModules,
    #[serde(default, skip_serializing_if = "V4StageModules::is_empty")]
    pub particle_spawn: V4StageModules,
    #[serde(default, skip_serializing_if = "V4StageModules::is_empty")]
    pub particle_update: V4StageModules,
}

impl V4StageModules {
    fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

/// An explicit simulation stage instance (§6.1): a stable id, its authored name, and its modules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V4SimulationStage {
    pub id: StageId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<V4Module>,
}

/// The v4 authored shape of one emitter: nested lifecycle containers + explicit simulation stages, and
/// a namespaced `domain` in place of `simulation_domain`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V4Emitter {
    pub id: EmitterId,
    pub name: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_color: Option<[f32; 4]>,
    pub transform: EmitterTransform,
    pub start_time: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_reference: Option<MarkerTimeReference>,
    pub duration: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<EmitterRegion>,
    pub max_particles: u32,
    pub domain: DomainTypeId,
    #[serde(default)]
    pub lifecycle: V4EmitterLifecycle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub simulation_stages: Vec<V4SimulationStage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renderers: Vec<RendererInstance>,
}

/// The full authored **format v4** document (extensible-stages M3). Serde DTO over the flat in-memory
/// [`EffectAsset`]: `from_effect` nests on save, `into_effect` flattens on load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthoredV4Document {
    pub format_version: u32,
    pub id: EffectId,
    pub name: String,
    pub duration: f32,
    // Accepts the legacy `looping: bool` spelling, like the flat model does.
    #[serde(
        default,
        alias = "looping",
        deserialize_with = "crate::model::deserialize_playback_mode"
    )]
    pub playback_mode: EffectPlaybackMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_transform_track: Option<HostTransformTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<AssetDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flipbooks: Vec<FlipbookDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<MaterialDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub material_instances: Vec<crate::material::MaterialInstance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<EffectParameter>,
    #[serde(default, skip_serializing_if = "V4EffectLifecycle::is_empty")]
    pub lifecycle: V4EffectLifecycle,
    #[serde(default)]
    pub emitters: Vec<V4Emitter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<EventLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<EffectMarker>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choreography_events: Vec<ChoreographyEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_clips: Vec<EffectClip>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choreography_order: Vec<ChoreographyTrackId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<AssetId>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl V4EffectLifecycle {
    fn is_empty(&self) -> bool {
        self.effect_spawn.is_empty() && self.effect_update.is_empty()
    }
}

fn domain_to_id(domain: &SimulationDomain) -> DomainTypeId {
    match domain {
        SimulationDomain::Particle => DomainTypeId::new(AESTRA_DOMAIN_PARTICLES),
        SimulationDomain::Strip => DomainTypeId::new(AESTRA_DOMAIN_STRIP),
        SimulationDomain::Custom(name) => DomainTypeId::new(name.clone()),
    }
}

fn id_to_domain(id: &DomainTypeId) -> SimulationDomain {
    match id.as_str() {
        AESTRA_DOMAIN_PARTICLES => SimulationDomain::Particle,
        AESTRA_DOMAIN_STRIP => SimulationDomain::Strip,
        other => SimulationDomain::Custom(other.to_string()),
    }
}

fn emitter_to_v4(emitter: &Emitter) -> Result<V4Emitter, V4ConversionError> {
    let mut lifecycle = V4EmitterLifecycle::default();
    let mut simulation_stages: Vec<V4SimulationStage> = Vec::new();
    for module in &emitter.modules {
        let entry = V4Module::from_module(module);
        match &module.stage {
            StageKind::EmitterSpawn => lifecycle.emitter_spawn.modules.push(entry),
            StageKind::EmitterUpdate => lifecycle.emitter_update.modules.push(entry),
            StageKind::ParticleSpawn => lifecycle.particle_spawn.modules.push(entry),
            StageKind::ParticleUpdate => lifecycle.particle_update.modules.push(entry),
            StageKind::Simulation(name) => {
                // First appearance of a name creates its stage (and fixes its order); later modules of
                // the same name append to it (§31.1).
                match simulation_stages.iter_mut().find(|s| &s.name == name) {
                    Some(stage) => stage.modules.push(entry),
                    None => simulation_stages.push(V4SimulationStage {
                        id: StageId::for_name(name),
                        name: name.clone(),
                        modules: vec![entry],
                    }),
                }
            }
            StageKind::EffectSpawn | StageKind::EffectUpdate => {
                return Err(V4ConversionError::EffectLevelModuleOnEmitter {
                    emitter: emitter.name.clone(),
                    module: module.module_type.clone(),
                });
            }
        }
    }
    Ok(V4Emitter {
        id: emitter.id,
        name: emitter.name.clone(),
        enabled: emitter.enabled,
        display_color: emitter.display_color,
        transform: emitter.transform,
        start_time: emitter.start_time,
        start_reference: emitter.start_reference,
        duration: emitter.duration,
        regions: emitter.regions.clone(),
        max_particles: emitter.max_particles,
        domain: domain_to_id(&emitter.simulation_domain),
        lifecycle,
        simulation_stages,
        renderers: emitter.renderers.clone(),
    })
}

fn emitter_from_v4(emitter: V4Emitter) -> Emitter {
    // Flatten the stage containers back into a flat module list in canonical stage order, restoring each
    // module's stage from its container.
    let mut modules = Vec::new();
    let mut push_stage = |bucket: Vec<V4Module>, stage: StageKind| {
        for module in bucket {
            modules.push(module.into_module(stage.clone()));
        }
    };
    push_stage(
        emitter.lifecycle.emitter_spawn.modules,
        StageKind::EmitterSpawn,
    );
    push_stage(
        emitter.lifecycle.emitter_update.modules,
        StageKind::EmitterUpdate,
    );
    push_stage(
        emitter.lifecycle.particle_spawn.modules,
        StageKind::ParticleSpawn,
    );
    push_stage(
        emitter.lifecycle.particle_update.modules,
        StageKind::ParticleUpdate,
    );
    for stage in emitter.simulation_stages {
        push_stage(stage.modules, StageKind::Simulation(stage.name));
    }
    Emitter {
        id: emitter.id,
        name: emitter.name,
        enabled: emitter.enabled,
        display_color: emitter.display_color,
        transform: emitter.transform,
        start_time: emitter.start_time,
        start_reference: emitter.start_reference,
        duration: emitter.duration,
        regions: emitter.regions,
        max_particles: emitter.max_particles,
        simulation_domain: id_to_domain(&emitter.domain),
        modules,
        renderers: emitter.renderers,
    }
}

impl AuthoredV4Document {
    /// Builds the v4 document from the flat in-memory effect, nesting each emitter's modules into their
    /// stage containers. Errors only when an emitter carries an effect-level module (unsupported today).
    pub fn from_effect(effect: &crate::EffectAsset) -> Result<Self, V4ConversionError> {
        let emitters = effect
            .emitters
            .iter()
            .map(emitter_to_v4)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            format_version: crate::CURRENT_FORMAT_VERSION,
            id: effect.id,
            name: effect.name.clone(),
            duration: effect.duration,
            playback_mode: effect.playback_mode,
            host_transform_track: effect.host_transform_track.clone(),
            assets: effect.assets.clone(),
            flipbooks: effect.flipbooks.clone(),
            materials: effect.materials.clone(),
            material_instances: effect.material_instances.clone(),
            parameters: effect.parameters.clone(),
            lifecycle: V4EffectLifecycle::default(),
            emitters,
            events: effect.events.clone(),
            markers: effect.markers.clone(),
            choreography_events: effect.choreography_events.clone(),
            effect_clips: effect.effect_clips.clone(),
            choreography_order: effect.choreography_order.clone(),
            dependencies: effect.dependencies.clone(),
            metadata: effect.metadata.clone(),
        })
    }

    /// Flattens the v4 document back into the in-memory effect, restoring each module's `stage` from its
    /// container and mapping `domain` back to a `SimulationDomain`.
    pub fn into_effect(self) -> crate::EffectAsset {
        crate::EffectAsset {
            format_version: self.format_version,
            id: self.id,
            name: self.name,
            duration: self.duration,
            playback_mode: self.playback_mode,
            host_transform_track: self.host_transform_track,
            assets: self.assets,
            flipbooks: self.flipbooks,
            materials: self.materials,
            material_instances: self.material_instances,
            parameters: self.parameters,
            emitters: self.emitters.into_iter().map(emitter_from_v4).collect(),
            events: self.events,
            markers: self.markers,
            choreography_events: self.choreography_events,
            effect_clips: self.effect_clips,
            choreography_order: self.choreography_order,
            dependencies: self.dependencies,
            metadata: self.metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ColorKey, Curve, CurveKey, DEFAULT_SPRITE_MATERIAL_ID, EffectAsset, EmitterShape, Gradient,
        MaterialDefinition, ModuleInstance, RendererInstance, ScalarRange,
    };

    /// A rich flat asset in canonical stage order, plus a custom simulation stage and a custom domain,
    /// to exercise every branch of the mapping.
    fn rich_effect() -> EffectAsset {
        let mut effect = EffectAsset::new("Rich", 3.0);
        effect.materials = vec![MaterialDefinition::default_sprite()];
        let mut emitter = Emitter {
            id: EmitterId::new(),
            name: "Emitter".into(),
            enabled: true,
            display_color: None,
            transform: EmitterTransform::default(),
            start_time: 0.0,
            start_reference: None,
            duration: 3.0,
            regions: Vec::new(),
            max_particles: 128,
            simulation_domain: SimulationDomain::Custom("org.example.domain/grid".into()),
            modules: vec![
                ModuleInstance::emission(24.0, 0),
                ModuleInstance::shape(EmitterShape::Point),
                ModuleInstance::initialize(
                    ScalarRange::new(0.8, 1.4),
                    ScalarRange::new(35.0, 70.0),
                    [0.0, 1.0, 0.0],
                    30.0,
                    ScalarRange::new(-1.0, 1.0),
                ),
                ModuleInstance::motion([0.0, -18.0, 0.0], 0.6, 4.0),
                ModuleInstance::appearance(
                    Curve::new(vec![CurveKey::new(0.0, 1.0)]),
                    Curve::new(vec![CurveKey::new(0.0, 1.0)]),
                    Gradient::new(vec![ColorKey::new(0.0, [1.0; 4])]),
                ),
            ],
            renderers: vec![RendererInstance::sprite(DEFAULT_SPRITE_MATERIAL_ID)],
        };
        // Two modules sharing one simulation-stage name, to exercise stage grouping.
        let mut solve_a = ModuleInstance::motion([0.0, 0.0, 0.0], 0.0, 0.0);
        solve_a.stage = StageKind::Simulation("solve".into());
        let mut solve_b = ModuleInstance::motion([1.0, 0.0, 0.0], 0.0, 0.0);
        solve_b.stage = StageKind::Simulation("solve".into());
        emitter.modules.push(solve_a);
        emitter.modules.push(solve_b);
        effect.emitters.push(emitter);
        effect
    }

    #[test]
    fn flat_to_v4_and_back_is_an_identity_for_canonically_ordered_assets() {
        let effect = rich_effect();
        let document = AuthoredV4Document::from_effect(&effect).expect("convert to v4");

        // The nesting is real: modules landed in their stage containers, not a flat list.
        let emitter = &document.emitters[0];
        assert_eq!(emitter.lifecycle.emitter_update.modules.len(), 1); // emission
        assert_eq!(emitter.lifecycle.particle_spawn.modules.len(), 2); // shape + initialize
        assert_eq!(emitter.lifecycle.particle_update.modules.len(), 2); // motion + appearance
        assert_eq!(
            emitter.simulation_stages.len(),
            1,
            "one 'solve' simulation stage"
        );
        assert_eq!(
            emitter.simulation_stages[0].modules.len(),
            2,
            "both solve modules grouped"
        );
        assert_eq!(emitter.simulation_stages[0].id, StageId::for_name("solve"));
        assert_eq!(emitter.domain, DomainTypeId::new("org.example.domain/grid"));

        let restored = document.into_effect();
        assert_eq!(effect, restored, "v4 round trip is an identity");
    }

    #[test]
    fn v4_document_serializes_and_deserializes_through_ron() {
        let effect = rich_effect();
        let document = AuthoredV4Document::from_effect(&effect).unwrap();
        let ron = ron::ser::to_string(&document).expect("serialize v4");
        let restored: AuthoredV4Document = ron::from_str(&ron).expect("deserialize v4");
        assert_eq!(document, restored, "v4 document round trips through RON");
        // The v4 file nests modules under named stages rather than a flat `modules:` list.
        assert!(ron.contains("particle_update"));
        assert!(ron.contains("simulation_stages"));
        assert!(ron.contains("domain"));
    }

    #[test]
    fn builtin_domains_round_trip_and_custom_is_preserved() {
        assert_eq!(
            id_to_domain(&domain_to_id(&SimulationDomain::Particle)),
            SimulationDomain::Particle
        );
        assert_eq!(
            id_to_domain(&domain_to_id(&SimulationDomain::Strip)),
            SimulationDomain::Strip
        );
        let custom = SimulationDomain::Custom("org.x.domain/foo".into());
        assert_eq!(id_to_domain(&domain_to_id(&custom)), custom);
        // A built-in maps to its reserved id, not a Custom.
        assert_eq!(
            domain_to_id(&SimulationDomain::Particle),
            DomainTypeId::new(AESTRA_DOMAIN_PARTICLES)
        );
    }

    #[test]
    fn effect_level_modules_on_an_emitter_are_reported() {
        let mut effect = rich_effect();
        let mut effect_module = ModuleInstance::emission(1.0, 0);
        effect_module.stage = StageKind::EffectUpdate;
        effect.emitters[0].modules.push(effect_module);
        assert!(matches!(
            AuthoredV4Document::from_effect(&effect),
            Err(V4ConversionError::EffectLevelModuleOnEmitter { .. })
        ));
    }
}
