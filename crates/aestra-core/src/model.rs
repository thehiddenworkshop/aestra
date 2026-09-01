use crate::{
    AssetId, ChoreographyEventId, CurveId, Diagnostic, DiagnosticCode, EffectClipId, EffectId,
    EmitterId, EmitterRegionId, EventId, GradientId, MarkerId, MaterialId, ModuleId, ParameterId,
    RendererId, ValidationReport,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};
use tempfile::NamedTempFile;
use thiserror::Error;

pub const MODULE_EMISSION: &str = "aestra.emission.rate";
pub const MODULE_SHAPE: &str = "aestra.spawn.shape";
pub const MODULE_INITIALIZE: &str = "aestra.spawn.initialize";
pub const MODULE_MOTION: &str = "aestra.update.motion";
pub const MODULE_APPEARANCE: &str = "aestra.update.appearance";
pub const RENDERER_SPRITE: &str = "aestra.renderer.sprite";
pub const RENDERER_FLIPBOOK: &str = "aestra.renderer.flipbook";
pub const RENDERER_RIBBON: &str = "aestra.renderer.ribbon";
pub const RENDERER_MESH: &str = "aestra.renderer.mesh";
pub const DEFAULT_SPRITE_MATERIAL_ID: MaterialId =
    MaterialId::from_u128(0xa357_4a00_0000_4000_8000_0000_0000_0001);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectAsset {
    pub format_version: u32,
    pub id: EffectId,
    pub name: String,
    pub duration: f32,
    #[serde(
        default = "EffectPlaybackMode::default",
        alias = "looping",
        deserialize_with = "deserialize_playback_mode"
    )]
    pub playback_mode: EffectPlaybackMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<AssetDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flipbooks: Vec<FlipbookDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<MaterialDefinition>,
    #[serde(default)]
    pub parameters: Vec<EffectParameter>,
    #[serde(default)]
    pub emitters: Vec<Emitter>,
    #[serde(default)]
    pub events: Vec<EventLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<EffectMarker>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choreography_events: Vec<ChoreographyEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_clips: Vec<EffectClip>,
    /// Optional stable presentation order for top-level choreography rows.
    /// Missing and stale entries are repaired by editor projections without changing runtime data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choreography_order: Vec<ChoreographyTrackId>,
    #[serde(default)]
    pub dependencies: Vec<AssetId>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl EffectAsset {
    pub fn new(name: impl Into<String>, duration: f32) -> Self {
        Self {
            format_version: crate::CURRENT_FORMAT_VERSION,
            id: EffectId::new(),
            name: name.into(),
            duration,
            playback_mode: EffectPlaybackMode::LoopRestart,
            assets: Vec::new(),
            flipbooks: Vec::new(),
            materials: vec![MaterialDefinition::default_sprite()],
            parameters: Vec::new(),
            emitters: Vec::new(),
            events: Vec::new(),
            markers: Vec::new(),
            choreography_events: Vec::new(),
            effect_clips: Vec::new(),
            choreography_order: Vec::new(),
            dependencies: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn validation_report(&self) -> ValidationReport {
        let mut report = ValidationReport::default();
        if self.format_version != crate::CURRENT_FORMAT_VERSION {
            report.push(Diagnostic::error(
                DiagnosticCode::UnsupportedFormat,
                "effect.format_version",
                format!(
                    "effect format version {} is unsupported; expected {}",
                    self.format_version,
                    crate::CURRENT_FORMAT_VERSION
                ),
            ));
        }
        if self.id.is_nil() {
            report.push(Diagnostic::error(
                DiagnosticCode::NilId,
                "effect.id",
                "effect ID cannot be nil",
            ));
        }
        if !self.duration.is_finite() || self.duration <= 0.0 {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidDuration,
                "effect.duration",
                format!(
                    "effect duration must be positive and finite, got {}",
                    self.duration
                ),
            ));
        }

        let mut semantic_ids = BTreeMap::<u128, String>::new();
        register_id(
            &mut report,
            &mut semantic_ids,
            self.id.as_uuid().as_u128(),
            "effect.id".into(),
        );
        for (index, asset) in self.assets.iter().enumerate() {
            asset.validate(
                &format!("effect.assets[{index}]"),
                &mut report,
                &mut semantic_ids,
            );
        }
        for (index, flipbook) in self.flipbooks.iter().enumerate() {
            flipbook.validate(
                &format!("effect.flipbooks[{index}]"),
                &mut report,
                &mut semantic_ids,
            );
        }
        for (index, material) in self.materials.iter().enumerate() {
            material.validate(
                &format!("effect.materials[{index}]"),
                &mut report,
                &mut semantic_ids,
            );
        }
        for (index, parameter) in self.parameters.iter().enumerate() {
            let path = format!("effect.parameters[{index}]");
            register_id(
                &mut report,
                &mut semantic_ids,
                parameter.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
            validate_value(
                &parameter.default,
                &format!("{path}.default"),
                &mut report,
                &mut semantic_ids,
            );
            if matches!(parameter.default, Value::Parameter(_)) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidValue,
                    format!("{path}.default"),
                    "effect parameter defaults must be concrete values",
                ));
            }
        }
        for (index, emitter) in self.emitters.iter().enumerate() {
            let emitter_path = format!("effect.emitters[{index}]");
            emitter.validate(&emitter_path, self.duration, &mut report, &mut semantic_ids);
            validate_marker_time_reference(
                emitter.start_reference,
                emitter.start_time,
                &format!("{emitter_path}.start_reference"),
                self,
                &mut report,
            );
            for (renderer_index, renderer) in emitter.renderers.iter().enumerate() {
                let path = format!("{emitter_path}.renderers[{renderer_index}].material");
                match self
                    .materials
                    .iter()
                    .find(|material| material.id == renderer.material)
                {
                    Some(material)
                        if matches!(
                            (&renderer.properties, &material.properties),
                            (
                                RendererProperties::Sprite,
                                MaterialProperties::Sprite { .. }
                            ) | (
                                RendererProperties::Flipbook { .. },
                                MaterialProperties::Sprite { .. }
                            )
                        ) => {}
                    Some(material) => report.push(Diagnostic::error(
                        DiagnosticCode::InvalidReference,
                        path,
                        format!(
                            "renderer type '{}' is incompatible with material '{}'",
                            renderer.renderer_type.0, material.name
                        ),
                    )),
                    None => report.push(Diagnostic::error(
                        DiagnosticCode::InvalidReference,
                        path,
                        format!("renderer references missing material {}", renderer.material),
                    )),
                }
            }
            for (module_index, module) in emitter.modules.iter().enumerate() {
                for (input, parameter_id) in &module.bindings {
                    let path = format!("{emitter_path}.modules[{module_index}].bindings.{input}");
                    let Some(expected) = module
                        .active_parameter_value(input)
                        .map(|value| value.value_type())
                        .or_else(|| module.parameter_type(input))
                    else {
                        report.push(Diagnostic::error(
                            DiagnosticCode::UnknownParameter,
                            path,
                            format!(
                                "module '{}' has no input named '{input}'",
                                module.module_type.0
                            ),
                        ));
                        continue;
                    };
                    let Some(parameter) = self
                        .parameters
                        .iter()
                        .find(|parameter| parameter.id == *parameter_id)
                    else {
                        report.push(Diagnostic::error(
                            DiagnosticCode::InvalidReference,
                            path,
                            format!("binding references missing parameter {parameter_id}"),
                        ));
                        continue;
                    };
                    let actual = parameter.default.value_type();
                    if actual != expected {
                        report.push(Diagnostic::error(
                            DiagnosticCode::ParameterTypeMismatch,
                            path,
                            format!(
                                "input '{input}' expects {expected:?}, but parameter '{}' is {actual:?}",
                                parameter.name
                            ),
                        ));
                    }
                }
            }
        }
        for (index, marker) in self.markers.iter().enumerate() {
            let path = format!("effect.markers[{index}]");
            register_id(
                &mut report,
                &mut semantic_ids,
                marker.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
            if marker.name.trim().is_empty() {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidValue,
                    format!("{path}.name"),
                    "marker name cannot be empty",
                ));
            }
            if !marker.time.is_finite() || !(0.0..=self.duration).contains(&marker.time) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidValue,
                    format!("{path}.time"),
                    format!(
                        "marker time must be finite and within 0..={}, got {}",
                        self.duration, marker.time
                    ),
                ));
            }
        }
        for (index, event) in self.choreography_events.iter().enumerate() {
            let path = format!("effect.choreography_events[{index}]");
            register_id(
                &mut report,
                &mut semantic_ids,
                event.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
            event.validate(&path, self, &mut report);
        }
        for (index, event) in self.events.iter().enumerate() {
            let path = format!("effect.events[{index}]");
            register_id(
                &mut report,
                &mut semantic_ids,
                event.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
            if !self.emitters.iter().any(|item| item.id == event.source) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidReference,
                    format!("{path}.source"),
                    format!("event references missing source emitter {}", event.source),
                ));
            }
            if !self.emitters.iter().any(|item| item.id == event.target) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidReference,
                    format!("{path}.target"),
                    format!("event references missing target emitter {}", event.target),
                ));
            }
        }
        for (index, clip) in self.effect_clips.iter().enumerate() {
            let path = format!("effect.effect_clips[{index}]");
            register_id(
                &mut report,
                &mut semantic_ids,
                clip.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
            clip.validate(&path, self, &mut report, &mut semantic_ids);
        }
        for (material_index, material) in self.materials.iter().enumerate() {
            let path = format!("effect.materials[{material_index}].properties");
            let MaterialProperties::Sprite {
                softness,
                color,
                texture,
                ..
            } = &material.properties;
            self.validate_material_input(
                softness,
                ValueType::Scalar,
                &format!("{path}.softness"),
                &mut report,
            );
            if let SpriteColorSource::Value(input) = color {
                self.validate_material_input(
                    input,
                    ValueType::Vec4,
                    &format!("{path}.color"),
                    &mut report,
                );
            }
            if let Some(input) = texture {
                match self.assets.iter().find(|asset| asset.id == *input) {
                    Some(asset) if asset.kind == AssetKind::Texture => {}
                    Some(asset) => report.push(Diagnostic::error(
                        DiagnosticCode::InvalidReference,
                        format!("{path}.texture"),
                        format!(
                            "material texture references '{}' which is registered as {:?}",
                            asset.name, asset.kind
                        ),
                    )),
                    None => report.push(Diagnostic::error(
                        DiagnosticCode::InvalidReference,
                        format!("{path}.texture"),
                        format!("material texture references missing asset {input}"),
                    )),
                }
            }
        }
        for (flipbook_index, flipbook) in self.flipbooks.iter().enumerate() {
            let path = format!("effect.flipbooks[{flipbook_index}].texture");
            match self
                .assets
                .iter()
                .find(|asset| asset.id == flipbook.texture)
            {
                Some(asset) if asset.kind == AssetKind::Texture => {}
                Some(asset) => report.push(Diagnostic::error(
                    DiagnosticCode::InvalidReference,
                    path,
                    format!(
                        "flipbook texture references '{}' which is registered as {:?}",
                        asset.name, asset.kind
                    ),
                )),
                None => report.push(Diagnostic::error(
                    DiagnosticCode::InvalidReference,
                    path,
                    format!(
                        "flipbook references missing texture asset {}",
                        flipbook.texture
                    ),
                )),
            }
        }
        for (emitter_index, emitter) in self.emitters.iter().enumerate() {
            for (renderer_index, renderer) in emitter.renderers.iter().enumerate() {
                if let RendererProperties::Flipbook { flipbook, .. } = renderer.properties
                    && !self.flipbooks.iter().any(|item| item.id == flipbook)
                {
                    report.push(Diagnostic::error(
                        DiagnosticCode::InvalidReference,
                        format!("effect.emitters[{emitter_index}].renderers[{renderer_index}].properties.flipbook"),
                        format!("renderer references missing flipbook {flipbook}"),
                    ));
                }
            }
        }
        report
    }

    fn validate_material_input<T>(
        &self,
        input: &MaterialInput<T>,
        expected: ValueType,
        path: &str,
        report: &mut ValidationReport,
    ) {
        let MaterialInput::Parameter(parameter_id) = input else {
            return;
        };
        let Some(parameter) = self
            .parameters
            .iter()
            .find(|parameter| parameter.id == *parameter_id)
        else {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidReference,
                path,
                format!("material input references missing parameter {parameter_id}"),
            ));
            return;
        };
        let actual = parameter.default.value_type();
        if actual != expected {
            report.push(Diagnostic::error(
                DiagnosticCode::ParameterTypeMismatch,
                path,
                format!("material input expects {expected:?}, found {actual:?}"),
            ));
        }
    }

    pub fn validate(&self) -> Result<(), ValidationReport> {
        self.validation_report().into_result()
    }

    pub fn from_ron(source: &str) -> Result<Self, AssetError> {
        let found = crate::detect_effect_format(source)?;
        if found != crate::CURRENT_FORMAT_VERSION {
            return Err(AssetError::UnsupportedFormat {
                found,
                current: crate::CURRENT_FORMAT_VERSION,
            });
        }
        let asset: Self = ron::from_str(source)?;
        asset.validate()?;
        Ok(asset)
    }

    pub fn load_ron(path: impl AsRef<Path>) -> Result<Self, AssetError> {
        Self::from_ron(&fs::read_to_string(path)?)
    }

    pub fn to_pretty_ron(&self) -> Result<String, AssetError> {
        self.validate()?;
        Ok(ron::ser::to_string_pretty(
            self,
            ron::ser::PrettyConfig::new().depth_limit(12),
        )?)
    }

    pub fn save_ron(&self, path: impl AsRef<Path>) -> Result<(), AssetError> {
        atomic_write(path.as_ref(), self.to_pretty_ron()?.as_bytes())?;
        Ok(())
    }
}

/// How an effect behaves when playback reaches its duration.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum EffectPlaybackMode {
    Once,
    #[default]
    LoopRestart,
    LoopContinuous,
}

impl EffectPlaybackMode {
    pub const fn is_looping(self) -> bool {
        !matches!(self, Self::Once)
    }

    pub const fn is_continuous(self) -> bool {
        matches!(self, Self::LoopContinuous)
    }
}

impl std::fmt::Display for EffectPlaybackMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Once => "Once",
            Self::LoopRestart => "Loop restart",
            Self::LoopContinuous => "Loop continuous",
        })
    }
}

fn deserialize_playback_mode<'de, D>(deserializer: D) -> Result<EffectPlaybackMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum CompatiblePlaybackMode {
        Legacy(bool),
        Current(EffectPlaybackMode),
    }

    Ok(match CompatiblePlaybackMode::deserialize(deserializer)? {
        CompatiblePlaybackMode::Legacy(false) => EffectPlaybackMode::Once,
        CompatiblePlaybackMode::Legacy(true) => EffectPlaybackMode::LoopRestart,
        CompatiblePlaybackMode::Current(mode) => mode,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectMarker {
    pub id: MarkerId,
    pub name: String,
    pub time: f32,
}

/// Optional semantic time anchor.
///
/// The owning object's resolved time remains the value consumed by runtimes; authoring commands
/// keep it in sync with `marker + offset` so compiled/runtime representations stay deterministic.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct MarkerTimeReference {
    pub marker: MarkerId,
    pub offset: f32,
}

impl MarkerTimeReference {
    pub const fn new(marker: MarkerId, offset: f32) -> Self {
        Self { marker, offset }
    }
}

fn validate_marker_time_reference(
    reference: Option<MarkerTimeReference>,
    resolved_time: f32,
    path: &str,
    owner: &EffectAsset,
    report: &mut ValidationReport,
) {
    let Some(reference) = reference else {
        return;
    };
    let Some(marker) = owner
        .markers
        .iter()
        .find(|marker| marker.id == reference.marker)
    else {
        report.push(Diagnostic::error(
            DiagnosticCode::InvalidReference,
            format!("{path}.marker"),
            format!("timing references missing marker {}", reference.marker),
        ));
        return;
    };
    if !reference.offset.is_finite() {
        report.push(Diagnostic::error(
            DiagnosticCode::InvalidTiming,
            format!("{path}.offset"),
            "marker-relative offset must be finite",
        ));
        return;
    }
    let expected = marker.time + reference.offset;
    if !expected.is_finite() || expected < 0.0 {
        report.push(Diagnostic::error(
            DiagnosticCode::InvalidTiming,
            path,
            "marker-relative time must resolve to a finite, non-negative time",
        ));
    } else if (expected - resolved_time).abs() > 1.0e-4 {
        report.push(Diagnostic::error(
            DiagnosticCode::InvalidTiming,
            path,
            format!("resolved time {resolved_time} does not match marker-relative time {expected}"),
        ));
    }
}

impl EffectMarker {
    pub fn new(name: impl Into<String>, time: f32) -> Self {
        Self {
            id: MarkerId::new(),
            name: name.into(),
            time,
        }
    }
}

/// A deterministic semantic notification authored at one point on the effect timeline.
/// It is intentionally separate from particle lifecycle routing (`EventLink`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChoreographyEvent {
    pub id: ChoreographyEventId,
    pub name: String,
    pub time: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_reference: Option<MarkerTimeReference>,
    pub payload: ChoreographyEventPayload,
}

impl ChoreographyEvent {
    pub fn new(name: impl Into<String>, time: f32, payload: ChoreographyEventPayload) -> Self {
        Self {
            id: ChoreographyEventId::new(),
            name: name.into(),
            time,
            time_reference: None,
            payload,
        }
    }

    fn validate(&self, path: &str, owner: &EffectAsset, report: &mut ValidationReport) {
        if self.name.trim().is_empty() {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.name"),
                "choreography event name cannot be empty",
            ));
        }
        if !self.time.is_finite() || !(0.0..=owner.duration).contains(&self.time) {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidTiming,
                format!("{path}.time"),
                "choreography event time must be finite and inside the effect duration",
            ));
        }
        validate_marker_time_reference(
            self.time_reference,
            self.time,
            &format!("{path}.time_reference"),
            owner,
            report,
        );
        if let ChoreographyEventPayload::CameraShake { intensity } = self.payload
            && (!intensity.is_finite() || intensity < 0.0)
        {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.payload.intensity"),
                "camera shake intensity must be finite and non-negative",
            ));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChoreographyEventPayload {
    GameplayNotify { topic: String },
    PlaySound { cue: String },
    CameraShake { intensity: f32 },
    SpawnChildEffect { effect: String },
}

impl ChoreographyEventPayload {
    pub const fn kind(&self) -> ChoreographyEventKind {
        match self {
            Self::GameplayNotify { .. } => ChoreographyEventKind::GameplayNotify,
            Self::PlaySound { .. } => ChoreographyEventKind::PlaySound,
            Self::CameraShake { .. } => ChoreographyEventKind::CameraShake,
            Self::SpawnChildEffect { .. } => ChoreographyEventKind::SpawnChildEffect,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChoreographyEventKind {
    GameplayNotify,
    PlaySound,
    CameraShake,
    SpawnChildEffect,
}

/// Stable identity of one top-level row in an effect choreography timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ChoreographyTrackId {
    EffectClip(EffectClipId),
    Emitter(EmitterId),
}

fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    // The temporary file lives beside the destination so persistence is a same-filesystem atomic
    // replacement. If writing or syncing fails, NamedTempFile removes it and leaves the previous
    // effect untouched.
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    let persisted = temporary.persist(path).map_err(|error| error.error)?;
    persisted.sync_all()?;

    // Persist the directory entry on platforms where directory handles can be synchronized.
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectParameter {
    pub id: ParameterId,
    pub name: String,
    pub default: Value,
    pub exposed: bool,
}

/// A stable, serializable reference to a reusable effect asset.
///
/// Project discovery and source-path resolution are deliberately owned by `aestra-project`;
/// this value lives in the semantic crate so authored assets can persist references without
/// introducing a dependency from the source model back to the project layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EffectAssetRef {
    pub id: EffectId,
}

impl EffectAssetRef {
    pub const fn new(id: EffectId) -> Self {
        Self { id }
    }
}

impl From<EffectId> for EffectAssetRef {
    fn from(id: EffectId) -> Self {
        Self::new(id)
    }
}

impl std::fmt::Display for EffectAssetRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id.fmt(formatter)
    }
}

/// Determines the deterministic random seed used by one reusable effect instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EffectClipSeed {
    /// Derive a stable child seed from the parent instance seed and clip identity.
    #[default]
    Inherit,
    /// Use the exact authored seed, independent of the parent instance.
    Fixed(u64),
}

impl EffectClipSeed {
    pub fn resolve(self, parent_seed: u64, clip: EffectClipId) -> u64 {
        match self {
            Self::Fixed(seed) => seed,
            Self::Inherit => {
                let id = clip.as_uuid().as_u128();
                mix_seed(parent_seed ^ id as u64 ^ (id >> 64) as u64)
            }
        }
    }
}

/// A timed, non-destructive instance of another project effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectClip {
    pub id: EffectClipId,
    pub source: EffectAssetRef,
    pub start_time: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_reference: Option<MarkerTimeReference>,
    pub source_offset: f32,
    pub duration: f32,
    /// Non-destructive transform applied to the complete referenced effect instance.
    #[serde(default, skip_serializing_if = "EmitterTransform::is_identity")]
    pub transform: EmitterTransform,
    #[serde(default)]
    pub seed: EffectClipSeed,
    /// Concrete values replacing exposed parameters on this referenced instance.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameter_overrides: BTreeMap<ParameterId, Value>,
}

impl EffectClip {
    pub fn new(source: impl Into<EffectAssetRef>, start_time: f32, duration: f32) -> Self {
        Self {
            id: EffectClipId::new(),
            source: source.into(),
            start_time,
            start_reference: None,
            source_offset: 0.0,
            duration,
            transform: EmitterTransform::default(),
            seed: EffectClipSeed::Inherit,
            parameter_overrides: BTreeMap::new(),
        }
    }

    /// Maps parent effect time into the referenced effect's local time.
    pub fn map_time(
        &self,
        parent_time: f32,
        source_duration: f32,
        source_looping: bool,
    ) -> Option<f32> {
        let elapsed = parent_time - self.start_time;
        if elapsed < 0.0 || elapsed > self.duration {
            return None;
        }
        let local = self.source_offset + elapsed;
        Some(if source_looping && source_duration > 0.0 {
            local.rem_euclid(source_duration)
        } else {
            local.clamp(0.0, source_duration.max(0.0))
        })
    }

    fn validate(
        &self,
        path: &str,
        owner: &EffectAsset,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        if self.source.id.is_nil() {
            report.push(Diagnostic::error(
                DiagnosticCode::NilId,
                format!("{path}.source"),
                "effect clip source ID cannot be nil",
            ));
        }
        if self.source.id == owner.id {
            report.push(Diagnostic::error(
                DiagnosticCode::ReferenceCycle,
                format!("{path}.source"),
                "effect cannot directly reference itself",
            ));
        }
        if !self.start_time.is_finite() || self.start_time < 0.0 {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidTiming,
                format!("{path}.start_time"),
                "effect clip start time must be finite and non-negative",
            ));
        }
        validate_marker_time_reference(
            self.start_reference,
            self.start_time,
            &format!("{path}.start_reference"),
            owner,
            report,
        );
        if !self.source_offset.is_finite() || self.source_offset < 0.0 {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidTiming,
                format!("{path}.source_offset"),
                "effect clip source offset must be finite and non-negative",
            ));
        }
        if !self.duration.is_finite() || self.duration <= 0.0 {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidDuration,
                format!("{path}.duration"),
                "effect clip duration must be positive and finite",
            ));
        } else if self.start_time.is_finite() && self.start_time + self.duration > owner.duration {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidTiming,
                path,
                "effect clip must fit inside the owning effect duration",
            ));
        }
        if !self.transform.is_valid() {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.transform"),
                "effect clip transform must be finite, have positive scale, and use a normalized quaternion",
            ));
        }
        for (parameter, value) in &self.parameter_overrides {
            let override_path = format!("{path}.parameter_overrides.{parameter}");
            validate_value(value, &override_path, report, semantic_ids);
            if matches!(value, Value::Parameter(_)) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidValue,
                    override_path,
                    "effect clip parameter overrides must be concrete values",
                ));
            }
        }
    }
}

fn mix_seed(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Emitter {
    pub id: EmitterId,
    pub name: String,
    pub enabled: bool,
    /// Optional authoring color used to identify this emitter in editor choreography views.
    /// Runtime simulation and rendering intentionally ignore this presentation hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_color: Option<[f32; 4]>,
    pub transform: EmitterTransform,
    pub start_time: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_reference: Option<MarkerTimeReference>,
    pub duration: f32,
    /// Explicit timeline occurrences of this emitter. An empty list preserves the legacy
    /// single-region contract represented by `start_time` and `duration`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<EmitterRegion>,
    pub max_particles: u32,
    pub simulation_domain: SimulationDomain,
    pub modules: Vec<ModuleInstance>,
    pub renderers: Vec<RendererInstance>,
}

/// One editable occurrence of an emitter definition on the effect timeline.
///
/// `source_offset` maps the region back into the emitter's original local-time interval. This
/// makes splitting and rejoining regions lossless and keeps emitter-time curves continuous.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct EmitterRegion {
    pub id: EmitterRegionId,
    pub start_time: f32,
    pub source_offset: f32,
    pub duration: f32,
}

impl EmitterRegion {
    pub fn new(start_time: f32, source_offset: f32, duration: f32) -> Self {
        Self {
            id: EmitterRegionId::new(),
            start_time,
            source_offset,
            duration,
        }
    }

    pub fn end_time(self) -> f32 {
        self.start_time + self.duration
    }

    pub fn source_end(self) -> f32 {
        self.source_offset + self.duration
    }
}

/// Authored local transform applied to one emitter before the effect-instance transform.
/// Rotation is stored as a normalized quaternion in `[x, y, z, w]` order.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct EmitterTransform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Default for EmitterTransform {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

impl EmitterTransform {
    fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    fn is_valid(&self) -> bool {
        let rotation_length_squared = self.rotation.iter().map(|value| value * value).sum::<f32>();
        self.translation
            .iter()
            .chain(self.rotation.iter())
            .chain(self.scale.iter())
            .all(|value| value.is_finite())
            && self.scale.iter().all(|value| *value > 0.0)
            && rotation_length_squared.is_finite()
            && (rotation_length_squared - 1.0).abs() <= 1.0e-3
    }
}

impl Emitter {
    pub fn basic_sprite(name: impl Into<String>, duration: f32) -> Self {
        Self {
            id: EmitterId::new(),
            name: name.into(),
            enabled: true,
            display_color: None,
            transform: EmitterTransform::default(),
            start_time: 0.0,
            start_reference: None,
            duration,
            regions: Vec::new(),
            max_particles: 128,
            simulation_domain: SimulationDomain::Particle,
            modules: vec![
                ModuleInstance::emission(24.0, 12),
                ModuleInstance::shape(EmitterShape::Circle { radius: 12.0 }),
                ModuleInstance::initialize(
                    ScalarRange::new(0.6, 1.2),
                    ScalarRange::new(35.0, 90.0),
                    [0.0, 1.0, 0.0],
                    180.0,
                    ScalarRange::new(-2.0, 2.0),
                ),
                ModuleInstance::motion([0.0, -18.0, 0.0], 0.6, 4.0),
                ModuleInstance::appearance(
                    Curve::new(vec![
                        CurveKey::new(0.0, 4.0),
                        CurveKey::new(0.35, 10.0),
                        CurveKey::new(1.0, 1.0),
                    ]),
                    Curve::new(vec![
                        CurveKey::new(0.0, 0.0),
                        CurveKey::new(0.12, 1.0),
                        CurveKey::new(1.0, 0.0),
                    ]),
                    Gradient::new(vec![
                        ColorKey::new(0.0, [0.35, 0.75, 1.0, 1.0]),
                        ColorKey::new(0.5, [0.62, 0.3, 1.0, 1.0]),
                        ColorKey::new(1.0, [0.15, 0.05, 0.4, 0.0]),
                    ]),
                ),
            ],
            renderers: vec![RendererInstance::sprite(DEFAULT_SPRITE_MATERIAL_ID)],
        }
    }

    pub fn regenerate_ids(&mut self) {
        self.id = EmitterId::new();
        for region in &mut self.regions {
            region.id = EmitterRegionId::new();
        }
        for module in &mut self.modules {
            module.regenerate_ids();
        }
        for renderer in &mut self.renderers {
            renderer.id = RendererId::new();
        }
    }

    pub fn implicit_region_id(&self) -> EmitterRegionId {
        const IMPLICIT_REGION_NAMESPACE: u128 = 0x9e37_79b9_7f4a_7c15_d1b5_4a32_d192_ed03;
        EmitterRegionId::from_u128(self.id.as_uuid().as_u128() ^ IMPLICIT_REGION_NAMESPACE)
    }

    pub fn timeline_regions(&self) -> Vec<EmitterRegion> {
        if self.regions.is_empty() {
            vec![EmitterRegion {
                id: self.implicit_region_id(),
                start_time: self.start_time,
                source_offset: 0.0,
                duration: self.duration,
            }]
        } else {
            self.regions.clone()
        }
    }

    pub fn timeline_region(&self, id: EmitterRegionId) -> Option<EmitterRegion> {
        if self.regions.is_empty() {
            (id == self.implicit_region_id()).then(|| self.timeline_regions()[0])
        } else {
            self.regions.iter().find(|region| region.id == id).copied()
        }
    }

    pub fn split_timeline_region(
        &self,
        id: EmitterRegionId,
        split_time: f32,
        right_id: EmitterRegionId,
    ) -> Option<Vec<EmitterRegion>> {
        let mut regions = self.timeline_regions();
        let index = regions.iter().position(|region| region.id == id)?;
        let region = regions[index];
        let left_duration = split_time - region.start_time;
        let right_duration = region.end_time() - split_time;
        if left_duration <= f32::EPSILON || right_duration <= f32::EPSILON {
            return None;
        }
        regions[index].duration = left_duration;
        regions.insert(
            index + 1,
            EmitterRegion {
                id: right_id,
                start_time: split_time,
                source_offset: region.source_offset + left_duration,
                duration: right_duration,
            },
        );
        Some(regions)
    }

    pub fn join_timeline_region_with_next(
        &self,
        id: EmitterRegionId,
    ) -> Option<Vec<EmitterRegion>> {
        let mut regions = self.timeline_regions();
        regions.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        let index = regions.iter().position(|region| region.id == id)?;
        let left = *regions.get(index)?;
        let right = *regions.get(index + 1)?;
        let contiguous_timeline = (left.end_time() - right.start_time).abs() <= 0.000_1;
        let contiguous_source = (left.source_end() - right.source_offset).abs() <= 0.000_1;
        if !contiguous_timeline || !contiguous_source {
            return None;
        }
        regions[index].duration += right.duration;
        regions.remove(index + 1);
        Some(self.normalize_timeline_regions(regions))
    }

    pub fn normalize_timeline_regions(&self, regions: Vec<EmitterRegion>) -> Vec<EmitterRegion> {
        if regions.len() == 1
            && regions[0].id == self.implicit_region_id()
            && (regions[0].start_time - self.start_time).abs() <= 0.000_1
            && regions[0].source_offset.abs() <= 0.000_1
            && (regions[0].duration - self.duration).abs() <= 0.000_1
        {
            Vec::new()
        } else {
            regions
        }
    }

    pub fn spawn_rate(&self) -> f32 {
        self.emission().0
    }

    pub fn spawn_rate_mut(&mut self) -> &mut f32 {
        match &mut self.module_mut(MODULE_EMISSION).parameters {
            ModuleParameters::Emission { spawn_rate, .. } => spawn_rate,
            _ => unreachable!("validated emission module has emission parameters"),
        }
    }

    pub fn burst_count(&self) -> u32 {
        self.emission().1
    }

    pub fn burst_count_mut(&mut self) -> &mut u32 {
        match &mut self.module_mut(MODULE_EMISSION).parameters {
            ModuleParameters::Emission { burst_count, .. } => burst_count,
            _ => unreachable!("validated emission module has emission parameters"),
        }
    }

    pub fn shape(&self) -> &EmitterShape {
        match &self.module(MODULE_SHAPE).parameters {
            ModuleParameters::Shape { shape } => shape,
            _ => unreachable!("validated shape module has shape parameters"),
        }
    }

    pub fn lifetime(&self) -> ScalarRange {
        self.initialize().0
    }

    pub fn lifetime_mut(&mut self) -> &mut ScalarRange {
        match &mut self.module_mut(MODULE_INITIALIZE).parameters {
            ModuleParameters::Initialize { lifetime, .. } => lifetime,
            _ => unreachable!("validated initialize module has initialize parameters"),
        }
    }

    pub fn speed(&self) -> ScalarRange {
        self.initialize().1
    }

    pub fn direction(&self) -> [f32; 3] {
        self.initialize().2
    }

    pub fn spread_degrees(&self) -> f32 {
        self.initialize().3
    }

    pub fn angular_velocity(&self) -> ScalarRange {
        self.initialize().4
    }

    pub fn gravity(&self) -> [f32; 3] {
        self.motion().0
    }

    pub fn drag(&self) -> f32 {
        self.motion().1
    }

    pub fn turbulence(&self) -> f32 {
        self.motion().2
    }

    pub fn size_curve(&self) -> &Curve {
        self.appearance().0
    }

    pub fn size_curve_mut(&mut self) -> &mut Curve {
        match &mut self.module_mut(MODULE_APPEARANCE).parameters {
            ModuleParameters::Appearance { size, .. } => size,
            _ => unreachable!("validated appearance module has appearance parameters"),
        }
    }

    pub fn opacity_curve(&self) -> &Curve {
        self.appearance().1
    }

    pub fn opacity_curve_mut(&mut self) -> &mut Curve {
        match &mut self.module_mut(MODULE_APPEARANCE).parameters {
            ModuleParameters::Appearance { opacity, .. } => opacity,
            _ => unreachable!("validated appearance module has appearance parameters"),
        }
    }

    pub fn color_gradient(&self) -> &Gradient {
        self.appearance().2
    }

    pub fn color_gradient_mut(&mut self) -> &mut Gradient {
        match &mut self.module_mut(MODULE_APPEARANCE).parameters {
            ModuleParameters::Appearance { color, .. } => color,
            _ => unreachable!("validated appearance module has appearance parameters"),
        }
    }

    pub fn module_by_type(&self, module_type: &str) -> Option<&ModuleInstance> {
        self.modules
            .iter()
            .find(|module| module.module_type.0 == module_type)
    }

    pub fn module_by_id(&self, id: ModuleId) -> Option<&ModuleInstance> {
        self.modules.iter().find(|module| module.id == id)
    }

    pub fn module_by_id_mut(&mut self, id: ModuleId) -> Option<&mut ModuleInstance> {
        self.modules.iter_mut().find(|module| module.id == id)
    }

    fn emission(&self) -> (f32, u32) {
        match &self.module(MODULE_EMISSION).parameters {
            ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            } => (*spawn_rate, *burst_count),
            _ => unreachable!("validated emission module has emission parameters"),
        }
    }

    fn initialize(&self) -> (ScalarRange, ScalarRange, [f32; 3], f32, ScalarRange) {
        match &self.module(MODULE_INITIALIZE).parameters {
            ModuleParameters::Initialize {
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
            } => (
                *lifetime,
                *speed,
                *direction,
                *spread_degrees,
                *angular_velocity,
            ),
            _ => unreachable!("validated initialize module has initialize parameters"),
        }
    }

    fn motion(&self) -> ([f32; 3], f32, f32) {
        match &self.module(MODULE_MOTION).parameters {
            ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            } => (*gravity, *drag, *turbulence),
            _ => unreachable!("validated motion module has motion parameters"),
        }
    }

    fn appearance(&self) -> (&Curve, &Curve, &Gradient) {
        match &self.module(MODULE_APPEARANCE).parameters {
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => (size, opacity, color),
            _ => unreachable!("validated appearance module has appearance parameters"),
        }
    }

    fn module(&self, module_type: &str) -> &ModuleInstance {
        self.modules
            .iter()
            .find(|module| module.module_type.0 == module_type)
            .unwrap_or_else(|| panic!("emitter is missing required module {module_type}"))
    }

    fn module_mut(&mut self, module_type: &str) -> &mut ModuleInstance {
        self.modules
            .iter_mut()
            .find(|module| module.module_type.0 == module_type)
            .unwrap_or_else(|| panic!("emitter is missing required module {module_type}"))
    }

    fn validate(
        &self,
        path: &str,
        effect_duration: f32,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        register_id(
            report,
            semantic_ids,
            self.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        if !self.start_time.is_finite()
            || self.start_time < 0.0
            || !self.duration.is_finite()
            || self.duration <= 0.0
            || self.start_time + self.duration > effect_duration + f32::EPSILON
        {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidTiming,
                path,
                format!("emitter '{}' has invalid timing", self.name),
            ));
        }
        for (index, region) in self.regions.iter().enumerate() {
            let region_path = format!("{path}.regions[{index}]");
            register_id(
                report,
                semantic_ids,
                region.id.as_uuid().as_u128(),
                format!("{region_path}.id"),
            );
            if !region.start_time.is_finite()
                || region.start_time < 0.0
                || !region.source_offset.is_finite()
                || region.source_offset < 0.0
                || !region.duration.is_finite()
                || region.duration <= 0.0
                || region.end_time() > effect_duration + f32::EPSILON
                || region.source_end() > self.duration + f32::EPSILON
            {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidTiming,
                    region_path,
                    format!(
                        "emitter region {} has invalid timeline or source timing",
                        region.id
                    ),
                ));
            }
        }
        if self.max_particles == 0 {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidCapacity,
                format!("{path}.max_particles"),
                "emitter capacity must be greater than zero",
            ));
        }
        if self.display_color.is_some_and(|color| {
            color
                .iter()
                .any(|component| !component.is_finite() || !(0.0..=1.0).contains(component))
        }) {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.display_color"),
                "emitter display color components must be finite and between zero and one",
            ));
        }
        if !self.transform.is_valid() {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.transform"),
                "emitter transform must be finite, have positive scale, and use a normalized quaternion",
            ));
        }

        for required in [
            MODULE_EMISSION,
            MODULE_SHAPE,
            MODULE_INITIALIZE,
            MODULE_MOTION,
            MODULE_APPEARANCE,
        ] {
            let count = self
                .modules
                .iter()
                .filter(|module| module.module_type.0 == required)
                .count();
            if count == 0 {
                report.push(Diagnostic::error(
                    DiagnosticCode::MissingModule,
                    format!("{path}.modules"),
                    format!("emitter is missing required module '{required}'"),
                ));
            }
        }

        for (index, module) in self.modules.iter().enumerate() {
            module.validate(&format!("{path}.modules[{index}]"), report, semantic_ids);
        }
        if self.renderers.is_empty() {
            report.push(Diagnostic::error(
                DiagnosticCode::MissingRenderer,
                format!("{path}.renderers"),
                "emitter must have at least one renderer",
            ));
        }
        for (index, renderer) in self.renderers.iter().enumerate() {
            renderer.validate(&format!("{path}.renderers[{index}]"), report, semantic_ids);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SimulationDomain {
    Particle,
    Strip,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StageKind {
    EffectSpawn,
    EffectUpdate,
    EmitterSpawn,
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
    Simulation(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct ModuleTypeId(pub String);

impl ModuleTypeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModuleInstance {
    pub id: ModuleId,
    pub module_type: ModuleTypeId,
    pub stage: StageKind,
    pub enabled: bool,
    pub parameters: ModuleParameters,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub property_sources: BTreeMap<String, PropertySource>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub property_source_values: BTreeMap<String, Vec<PropertySourceValue>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, ParameterId>,
}

impl ModuleInstance {
    pub fn emission(spawn_rate: f32, burst_count: u32) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_EMISSION),
            stage: StageKind::EmitterUpdate,
            enabled: true,
            parameters: ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            },
            property_sources: BTreeMap::new(),
            property_source_values: BTreeMap::new(),
            bindings: BTreeMap::new(),
        }
    }

    pub fn shape(shape: EmitterShape) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_SHAPE),
            stage: StageKind::ParticleSpawn,
            enabled: true,
            parameters: ModuleParameters::Shape { shape },
            property_sources: BTreeMap::new(),
            property_source_values: BTreeMap::new(),
            bindings: BTreeMap::new(),
        }
    }

    pub fn initialize(
        lifetime: ScalarRange,
        speed: ScalarRange,
        direction: [f32; 3],
        spread_degrees: f32,
        angular_velocity: ScalarRange,
    ) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_INITIALIZE),
            stage: StageKind::ParticleSpawn,
            enabled: true,
            parameters: ModuleParameters::Initialize {
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
            },
            property_sources: BTreeMap::new(),
            property_source_values: BTreeMap::new(),
            bindings: BTreeMap::new(),
        }
    }

    pub fn motion(gravity: [f32; 3], drag: f32, turbulence: f32) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_MOTION),
            stage: StageKind::ParticleUpdate,
            enabled: true,
            parameters: ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            },
            property_sources: BTreeMap::new(),
            property_source_values: BTreeMap::new(),
            bindings: BTreeMap::new(),
        }
    }

    pub fn appearance(size: Curve, opacity: Curve, color: Gradient) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_APPEARANCE),
            stage: StageKind::ParticleUpdate,
            enabled: true,
            parameters: ModuleParameters::Appearance {
                size,
                opacity,
                color,
            },
            property_sources: BTreeMap::new(),
            property_source_values: BTreeMap::new(),
            bindings: BTreeMap::new(),
        }
    }

    /// Assign fresh semantic IDs to a cloned module and its nested authored values.
    pub fn regenerate_ids(&mut self) {
        self.id = ModuleId::new();
        match &mut self.parameters {
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => {
                size.id = CurveId::new();
                opacity.id = CurveId::new();
                color.id = GradientId::new();
            }
            ModuleParameters::Custom(values) => {
                for value in values.values_mut() {
                    value.regenerate_ids();
                }
            }
            _ => {}
        }
        for values in self.property_source_values.values_mut() {
            for value in values {
                value.value.regenerate_ids();
            }
        }
    }

    pub fn parameter_type(&self, parameter: &str) -> Option<ValueType> {
        match (&self.parameters, parameter) {
            (ModuleParameters::Emission { .. }, "spawn_rate") => Some(ValueType::Scalar),
            (ModuleParameters::Emission { .. }, "burst_count") => Some(ValueType::U32),
            (ModuleParameters::Shape { .. }, "shape") => Some(ValueType::Shape),
            (ModuleParameters::Initialize { .. }, "lifetime" | "speed" | "angular_velocity") => {
                Some(ValueType::Range)
            }
            (ModuleParameters::Initialize { .. }, "direction") => Some(ValueType::Vec3),
            (ModuleParameters::Initialize { .. }, "spread_degrees") => Some(ValueType::Scalar),
            (ModuleParameters::Motion { .. }, "gravity") => Some(ValueType::Vec3),
            (ModuleParameters::Motion { .. }, "drag" | "turbulence") => Some(ValueType::Scalar),
            (ModuleParameters::Appearance { .. }, "size" | "opacity") => Some(ValueType::Curve),
            (ModuleParameters::Appearance { .. }, "color") => Some(ValueType::Gradient),
            (ModuleParameters::Custom(values), name) => values.get(name).map(Value::value_type),
            _ => None,
        }
    }

    pub fn parameter_value(&self, parameter: &str) -> Option<Value> {
        match (&self.parameters, parameter) {
            (ModuleParameters::Emission { spawn_rate, .. }, "spawn_rate") => {
                Some(Value::Scalar(*spawn_rate))
            }
            (ModuleParameters::Emission { burst_count, .. }, "burst_count") => {
                Some(Value::U32(*burst_count))
            }
            (ModuleParameters::Shape { shape }, "shape") => Some(Value::Shape(*shape)),
            (ModuleParameters::Initialize { lifetime, .. }, "lifetime") => {
                Some(Value::Range(*lifetime))
            }
            (ModuleParameters::Initialize { speed, .. }, "speed") => Some(Value::Range(*speed)),
            (ModuleParameters::Initialize { direction, .. }, "direction") => {
                Some(Value::Vec3(*direction))
            }
            (ModuleParameters::Initialize { spread_degrees, .. }, "spread_degrees") => {
                Some(Value::Scalar(*spread_degrees))
            }
            (
                ModuleParameters::Initialize {
                    angular_velocity, ..
                },
                "angular_velocity",
            ) => Some(Value::Range(*angular_velocity)),
            (ModuleParameters::Motion { gravity, .. }, "gravity") => Some(Value::Vec3(*gravity)),
            (ModuleParameters::Motion { drag, .. }, "drag") => Some(Value::Scalar(*drag)),
            (ModuleParameters::Motion { turbulence, .. }, "turbulence") => {
                Some(Value::Scalar(*turbulence))
            }
            (ModuleParameters::Appearance { size, .. }, "size") => Some(Value::Curve(size.clone())),
            (ModuleParameters::Appearance { opacity, .. }, "opacity") => {
                Some(Value::Curve(opacity.clone()))
            }
            (ModuleParameters::Appearance { color, .. }, "color") => {
                Some(Value::Gradient(color.clone()))
            }
            (ModuleParameters::Custom(values), name) => values.get(name).cloned(),
            _ => None,
        }
    }

    pub fn property_source(&self, parameter: &str) -> Option<PropertySource> {
        self.property_sources.get(parameter).copied().or_else(|| {
            self.parameter_value(parameter)
                .as_ref()
                .map(PropertySource::infer_legacy)
        })
    }

    pub fn property_value_for_source(
        &self,
        parameter: &str,
        source: PropertySource,
    ) -> Option<Value> {
        if source != PropertySource::Constant
            && let Some(value) = self
                .property_source_values
                .get(parameter)
                .and_then(|values| values.iter().find(|value| value.source == source))
        {
            return Some(value.value.clone());
        }
        self.parameter_value(parameter)
            .filter(|value| source.accepts(value))
    }

    pub fn active_parameter_value(&self, parameter: &str) -> Option<Value> {
        let source = self.property_source(parameter)?;
        self.property_value_for_source(parameter, source)
    }

    pub fn property_source_value_mut(
        &mut self,
        parameter: &str,
        source: PropertySource,
    ) -> Option<&mut Value> {
        self.property_source_values
            .get_mut(parameter)?
            .iter_mut()
            .find(|value| value.source == source)
            .map(|value| &mut value.value)
    }

    fn validate(
        &self,
        path: &str,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        register_id(
            report,
            semantic_ids,
            self.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        for (parameter, source) in &self.property_sources {
            let source_path = format!("{path}.property_sources.{parameter}");
            let Some(value) = self.property_value_for_source(parameter, *source) else {
                invalid_value(
                    report,
                    &source_path,
                    "property source has no compatible authored value",
                );
                continue;
            };
            if !source.accepts(&value) {
                invalid_value(
                    report,
                    &source_path,
                    "property source is incompatible with the authored value type",
                );
            }
        }
        for (parameter, values) in &self.property_source_values {
            if self.parameter_value(parameter).is_none() {
                invalid_value(
                    report,
                    &format!("{path}.property_source_values.{parameter}"),
                    "property source values reference an unknown module parameter",
                );
                continue;
            }
            let mut sources = BTreeSet::new();
            for (index, source_value) in values.iter().enumerate() {
                let value_path = format!("{path}.property_source_values.{parameter}[{index}]");
                if source_value.source == PropertySource::Constant {
                    invalid_value(
                        report,
                        &value_path,
                        "constant values are stored in the module parameter",
                    );
                }
                if !sources.insert(source_value.source) {
                    invalid_value(report, &value_path, "property source value is duplicated");
                }
                if !source_value.source.accepts(&source_value.value) {
                    invalid_value(
                        report,
                        &value_path,
                        "property source value is incompatible with its source",
                    );
                }
                validate_value(
                    &source_value.value,
                    &format!("{value_path}.value"),
                    report,
                    semantic_ids,
                );
            }
        }
        let expected = match &self.parameters {
            ModuleParameters::Emission { .. } => (MODULE_EMISSION, StageKind::EmitterUpdate),
            ModuleParameters::Shape { .. } => (MODULE_SHAPE, StageKind::ParticleSpawn),
            ModuleParameters::Initialize { .. } => (MODULE_INITIALIZE, StageKind::ParticleSpawn),
            ModuleParameters::Motion { .. } => (MODULE_MOTION, StageKind::ParticleUpdate),
            ModuleParameters::Appearance { .. } => (MODULE_APPEARANCE, StageKind::ParticleUpdate),
            ModuleParameters::Custom(values) => {
                if self.module_type.0.trim().is_empty() {
                    invalid_value(report, path, "custom module type cannot be empty");
                }
                for (name, value) in values {
                    validate_value(
                        value,
                        &format!("{path}.parameters.{name}"),
                        report,
                        semantic_ids,
                    );
                }
                return;
            }
        };
        if self.module_type.0 != expected.0 {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.module_type"),
                format!(
                    "module parameters require type '{}', found '{}'",
                    expected.0, self.module_type.0
                ),
            ));
        }
        if self.stage != expected.1 {
            report.push(Diagnostic::error(
                DiagnosticCode::StageMismatch,
                format!("{path}.stage"),
                format!(
                    "module '{}' is assigned to the wrong stage",
                    self.module_type.0
                ),
            ));
        }
        match &self.parameters {
            ModuleParameters::Emission { spawn_rate, .. }
                if !spawn_rate.is_finite() || *spawn_rate < 0.0 =>
            {
                invalid_value(report, path, "spawn rate must be finite and non-negative");
            }
            ModuleParameters::Shape { shape } => {
                validate_value(
                    &Value::Shape(*shape),
                    &format!("{path}.shape"),
                    report,
                    semantic_ids,
                );
            }
            ModuleParameters::Initialize {
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
            } => {
                validate_range(*lifetime, path, "lifetime", report);
                validate_range(*speed, path, "speed", report);
                validate_range(*angular_velocity, path, "angular_velocity", report);
                if direction.iter().any(|value| !value.is_finite())
                    || !spread_degrees.is_finite()
                    || !(0.0..=360.0).contains(spread_degrees)
                    || direction.iter().all(|value| value.abs() <= f32::EPSILON)
                {
                    invalid_value(
                        report,
                        path,
                        "direction must be finite and non-zero, and spread must be between 0 and 360 degrees",
                    );
                }
            }
            ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            } if gravity.iter().any(|value| !value.is_finite())
                || !drag.is_finite()
                || *drag < 0.0
                || !turbulence.is_finite() =>
            {
                invalid_value(
                    report,
                    path,
                    "motion parameters must be finite and drag non-negative",
                );
            }
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => {
                validate_curve(size, &format!("{path}.size"), report);
                validate_curve(opacity, &format!("{path}.opacity"), report);
                validate_gradient(color, &format!("{path}.color"), report);
                register_id(
                    report,
                    semantic_ids,
                    size.id.as_uuid().as_u128(),
                    format!("{path}.size.id"),
                );
                register_id(
                    report,
                    semantic_ids,
                    opacity.id.as_uuid().as_u128(),
                    format!("{path}.opacity.id"),
                );
                register_id(
                    report,
                    semantic_ids,
                    color.id.as_uuid().as_u128(),
                    format!("{path}.color.id"),
                );
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModuleParameters {
    Emission {
        spawn_rate: f32,
        burst_count: u32,
    },
    Shape {
        shape: EmitterShape,
    },
    Initialize {
        lifetime: ScalarRange,
        speed: ScalarRange,
        direction: [f32; 3],
        spread_degrees: f32,
        angular_velocity: ScalarRange,
    },
    Motion {
        gravity: [f32; 3],
        drag: f32,
        turbulence: f32,
    },
    Appearance {
        size: Curve,
        opacity: Curve,
        color: Gradient,
    },
    Custom(BTreeMap<String, Value>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Value {
    Bool(bool),
    U32(u32),
    Scalar(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec3Range(Vec3Range),
    Vec3Curve(Vec3Curve),
    Vec4([f32; 4]),
    Text(String),
    Range(ScalarRange),
    Curve(Curve),
    Gradient(Gradient),
    Shape(EmitterShape),
    Parameter(ParameterId),
    Asset(AssetId),
    Material(MaterialId),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropertyEvaluationDomain {
    ParticleLife,
    EmitterTime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropertySource {
    Constant,
    RandomRange,
    Curve(PropertyEvaluationDomain),
    Gradient(PropertyEvaluationDomain),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PropertySourceValue {
    pub source: PropertySource,
    pub value: Value,
}

impl PropertySourceValue {
    pub fn new(source: PropertySource, value: Value) -> Self {
        Self { source, value }
    }
}

impl PropertySource {
    pub fn infer_legacy(value: &Value) -> Self {
        match value {
            Value::Range(range) if (range.max - range.min).abs() > f32::EPSILON => {
                Self::RandomRange
            }
            Value::Vec3Range(range) if range.min != range.max => Self::RandomRange,
            Value::Curve(curve) if curve.keys.len() > 1 => {
                Self::Curve(PropertyEvaluationDomain::ParticleLife)
            }
            Value::Vec3Curve(curve) if curve.curves.iter().any(|axis| axis.keys.len() > 1) => {
                Self::Curve(PropertyEvaluationDomain::ParticleLife)
            }
            Value::Gradient(gradient) if gradient.keys.len() > 1 => {
                Self::Gradient(PropertyEvaluationDomain::ParticleLife)
            }
            _ => Self::Constant,
        }
    }

    pub fn accepts(self, value: &Value) -> bool {
        matches!(
            (self, value),
            (Self::Constant, _)
                | (Self::RandomRange, Value::Range(_) | Value::Vec3Range(_))
                | (Self::Curve(_), Value::Curve(_) | Value::Vec3Curve(_))
                | (Self::Gradient(_), Value::Gradient(_))
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ValueType {
    Bool,
    U32,
    Scalar,
    Vec2,
    Vec3,
    Vec3Range,
    Vec3Curve,
    Vec4,
    Text,
    Range,
    Curve,
    Gradient,
    Shape,
    Parameter,
    Asset,
    Material,
}

impl Value {
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::U32(_) => ValueType::U32,
            Self::Scalar(_) => ValueType::Scalar,
            Self::Vec2(_) => ValueType::Vec2,
            Self::Vec3(_) => ValueType::Vec3,
            Self::Vec3Range(_) => ValueType::Vec3Range,
            Self::Vec3Curve(_) => ValueType::Vec3Curve,
            Self::Vec4(_) => ValueType::Vec4,
            Self::Text(_) => ValueType::Text,
            Self::Range(_) => ValueType::Range,
            Self::Curve(_) => ValueType::Curve,
            Self::Gradient(_) => ValueType::Gradient,
            Self::Shape(_) => ValueType::Shape,
            Self::Parameter(_) => ValueType::Parameter,
            Self::Asset(_) => ValueType::Asset,
            Self::Material(_) => ValueType::Material,
        }
    }

    /// Gives nested curve and gradient payloads fresh semantic identities when a value is copied
    /// into a second authored storage location.
    pub fn regenerate_ids(&mut self) {
        match self {
            Value::Curve(curve) => curve.id = CurveId::new(),
            Value::Vec3Curve(curve) => {
                for axis in &mut curve.curves {
                    axis.id = CurveId::new();
                }
            }
            Value::Gradient(gradient) => gradient.id = GradientId::new(),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum EmitterShape {
    Point,
    Circle { radius: f32 },
    Ring { radius: f32 },
    Sphere { radius: f32 },
    Hemisphere { radius: f32 },
    Box { half_extents: [f32; 3] },
    Cylinder { radius: f32, depth: f32 },
    Cone { radius: f32, depth: f32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlendMode {
    Alpha,
    Additive,
    Multiply,
}

/// The engine-independent type of an entry in an effect's asset registry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetKind {
    Texture,
    Mesh,
    Flipbook,
}

/// A stable asset identity and its project-relative source path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetDefinition {
    pub id: AssetId,
    pub name: String,
    pub kind: AssetKind,
    pub path: String,
}

impl AssetDefinition {
    pub fn texture(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: AssetId::new(),
            name: name.into(),
            kind: AssetKind::Texture,
            path: path.into(),
        }
    }

    fn validate(
        &self,
        path: &str,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        register_id(
            report,
            semantic_ids,
            self.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        if self.name.trim().is_empty() {
            invalid_value(
                report,
                &format!("{path}.name"),
                "asset name cannot be empty",
            );
        }
        let source = Path::new(&self.path);
        let unsafe_component = source.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        });
        if self.path.trim().is_empty() || source.is_absolute() || unsafe_component {
            invalid_value(
                report,
                &format!("{path}.path"),
                "asset path must be a non-empty project-relative path without parent traversal",
            );
        }
    }
}

/// Imported sprite-sheet metadata. Frames are explicit normalized UV rectangles,
/// so importers can support packed atlases as well as regular grids.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlipbookDefinition {
    pub id: AssetId,
    pub name: String,
    pub texture: AssetId,
    pub frames: Vec<UvRect>,
    pub frame_rate: f32,
    pub looping: bool,
}

impl FlipbookDefinition {
    pub fn grid(
        name: impl Into<String>,
        texture: AssetId,
        columns: u32,
        rows: u32,
        frame_rate: f32,
    ) -> Self {
        let columns = columns.max(1);
        let rows = rows.max(1);
        let mut frames = Vec::with_capacity((columns * rows) as usize);
        for row in 0..rows {
            for column in 0..columns {
                frames.push(UvRect {
                    min: [column as f32 / columns as f32, row as f32 / rows as f32],
                    max: [
                        (column + 1) as f32 / columns as f32,
                        (row + 1) as f32 / rows as f32,
                    ],
                });
            }
        }
        Self {
            id: AssetId::new(),
            name: name.into(),
            texture,
            frames,
            frame_rate,
            looping: true,
        }
    }

    fn validate(
        &self,
        path: &str,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        register_id(
            report,
            semantic_ids,
            self.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        if self.name.trim().is_empty() {
            invalid_value(
                report,
                &format!("{path}.name"),
                "flipbook name cannot be empty",
            );
        }
        if self.texture.is_nil() {
            invalid_value(
                report,
                &format!("{path}.texture"),
                "flipbook texture cannot be nil",
            );
        }
        if self.frames.is_empty() {
            invalid_value(
                report,
                &format!("{path}.frames"),
                "flipbook must contain at least one frame",
            );
        }
        for (index, frame) in self.frames.iter().enumerate() {
            if !frame.is_valid() {
                invalid_value(
                    report,
                    &format!("{path}.frames[{index}]"),
                    "flipbook frame UV bounds must be finite, normalized, and have positive area",
                );
            }
        }
        if !self.frame_rate.is_finite() || self.frame_rate <= 0.0 {
            invalid_value(
                report,
                &format!("{path}.frame_rate"),
                "flipbook frame rate must be positive and finite",
            );
        }
    }
}

/// Normalized texture coordinates used by a sprite renderer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct UvRect {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl UvRect {
    pub const FULL: Self = Self {
        min: [0.0, 0.0],
        max: [1.0, 1.0],
    };

    fn is_full(value: &Self) -> bool {
        *value == Self::FULL
    }

    fn is_valid(self) -> bool {
        self.min.into_iter().chain(self.max).all(f32::is_finite)
            && self.min[0] >= 0.0
            && self.min[1] >= 0.0
            && self.max[0] <= 1.0
            && self.max[1] <= 1.0
            && self.min[0] < self.max[0]
            && self.min[1] < self.max[1]
    }
}

impl Default for UvRect {
    fn default() -> Self {
        Self::FULL
    }
}

/// A constant material input or a typed reference to an exposed effect parameter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MaterialInput<T> {
    Constant(T),
    Parameter(ParameterId),
}

impl<T> MaterialInput<T> {
    pub fn constant(&self) -> Option<&T> {
        match self {
            Self::Constant(value) => Some(value),
            Self::Parameter(_) => None,
        }
    }
}

/// Controls whether the sprite material consumes the simulated particle color or
/// a typed constant/parameter value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SpriteColorSource {
    ParticleColor,
    Value(MaterialInput<[f32; 4]>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MaterialProperties {
    Sprite {
        softness: MaterialInput<f32>,
        color: SpriteColorSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        texture: Option<AssetId>,
        #[serde(default, skip_serializing_if = "UvRect::is_full")]
        uv: UvRect,
    },
}

/// An engine-independent material asset referenced by renderers through a stable ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterialDefinition {
    pub id: MaterialId,
    pub name: String,
    pub blend: BlendMode,
    pub properties: MaterialProperties,
}

impl MaterialDefinition {
    pub fn sprite(name: impl Into<String>, blend: BlendMode, softness: f32) -> Self {
        Self {
            id: MaterialId::new(),
            name: name.into(),
            blend,
            properties: MaterialProperties::Sprite {
                softness: MaterialInput::Constant(softness),
                color: SpriteColorSource::ParticleColor,
                texture: None,
                uv: UvRect::FULL,
            },
        }
    }

    pub fn default_sprite() -> Self {
        Self {
            id: DEFAULT_SPRITE_MATERIAL_ID,
            name: "Default Sprite".into(),
            blend: BlendMode::Additive,
            properties: MaterialProperties::Sprite {
                softness: MaterialInput::Constant(0.5),
                color: SpriteColorSource::ParticleColor,
                texture: None,
                uv: UvRect::FULL,
            },
        }
    }

    fn validate(
        &self,
        path: &str,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        register_id(
            report,
            semantic_ids,
            self.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        if self.name.trim().is_empty() {
            invalid_value(
                report,
                &format!("{path}.name"),
                "material name cannot be empty",
            );
        }
        let MaterialProperties::Sprite {
            softness,
            color,
            texture,
            uv,
        } = &self.properties;
        if softness
            .constant()
            .is_some_and(|value| !value.is_finite() || *value < 0.0)
        {
            invalid_value(
                report,
                &format!("{path}.properties.softness"),
                "sprite material softness must be finite and non-negative",
            );
        }
        if let SpriteColorSource::Value(MaterialInput::Constant(value)) = color
            && value.iter().any(|component| !component.is_finite())
        {
            invalid_value(
                report,
                &format!("{path}.properties.color"),
                "sprite material color must be finite",
            );
        }
        if texture.is_some_and(AssetId::is_nil) {
            invalid_value(
                report,
                &format!("{path}.properties.texture"),
                "sprite material texture asset cannot be nil",
            );
        }
        if !uv.is_valid() {
            invalid_value(
                report,
                &format!("{path}.properties.uv"),
                "sprite material UV bounds must be finite, normalized, and have positive area",
            );
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct RendererTypeId(pub String);

impl RendererTypeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RendererInstance {
    pub id: RendererId,
    pub renderer_type: RendererTypeId,
    pub enabled: bool,
    pub material: MaterialId,
    pub properties: RendererProperties,
}

impl RendererInstance {
    pub fn sprite(material: MaterialId) -> Self {
        Self {
            id: RendererId::new(),
            renderer_type: RendererTypeId::new(RENDERER_SPRITE),
            enabled: true,
            material,
            properties: RendererProperties::Sprite,
        }
    }

    pub fn flipbook(material: MaterialId, flipbook: AssetId) -> Self {
        Self {
            id: RendererId::new(),
            renderer_type: RendererTypeId::new(RENDERER_FLIPBOOK),
            enabled: true,
            material,
            properties: RendererProperties::Flipbook {
                flipbook,
                time_source: FlipbookTimeSource::ParticleAge,
                playback: FlipbookPlaybackMode::Forward,
                random_start: false,
            },
        }
    }

    fn validate(
        &self,
        path: &str,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        register_id(
            report,
            semantic_ids,
            self.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        if self.material.is_nil() {
            invalid_value(
                report,
                &format!("{path}.material"),
                "renderer material cannot be nil",
            );
        }
        let expected_type = match &self.properties {
            RendererProperties::Sprite => Some(RENDERER_SPRITE),
            RendererProperties::Flipbook { flipbook, .. } => {
                if flipbook.is_nil() {
                    invalid_value(report, path, "flipbook renderer asset cannot be nil");
                }
                Some(RENDERER_FLIPBOOK)
            }
            RendererProperties::Ribbon { width } => {
                if !width.is_finite() || *width <= 0.0 {
                    invalid_value(report, path, "ribbon width must be positive and finite");
                }
                Some(RENDERER_RIBBON)
            }
            RendererProperties::Mesh { asset } => {
                if asset.is_nil() {
                    invalid_value(report, path, "mesh renderer asset cannot be nil");
                }
                Some(RENDERER_MESH)
            }
            RendererProperties::Custom(values) => {
                if self.renderer_type.0.trim().is_empty() {
                    invalid_value(report, path, "custom renderer type cannot be empty");
                }
                for (name, value) in values {
                    validate_value(
                        value,
                        &format!("{path}.properties.{name}"),
                        report,
                        semantic_ids,
                    );
                }
                None
            }
        };
        if let Some(expected_type) = expected_type
            && self.renderer_type.0 != expected_type
        {
            invalid_value(
                report,
                &format!("{path}.renderer_type"),
                &format!(
                    "renderer properties require type '{expected_type}', found '{}'",
                    self.renderer_type.0
                ),
            );
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RendererProperties {
    Sprite,
    Flipbook {
        flipbook: AssetId,
        time_source: FlipbookTimeSource,
        playback: FlipbookPlaybackMode,
        random_start: bool,
    },
    Ribbon {
        width: f32,
    },
    Mesh {
        asset: AssetId,
    },
    Custom(BTreeMap<String, Value>),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FlipbookTimeSource {
    ParticleAge,
    EffectTime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FlipbookPlaybackMode {
    Forward,
    Reverse,
    PingPong,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventLink {
    pub id: EventId,
    pub source: EmitterId,
    pub trigger: EventTrigger,
    pub target: EmitterId,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventTrigger {
    OnSpawn,
    OnDeath,
    OnCollision,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ScalarRange {
    pub min: f32,
    pub max: f32,
}

impl ScalarRange {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    pub fn sample(self, random: f32) -> f32 {
        self.min + (self.max - self.min) * random.clamp(0.0, 1.0)
    }
}

/// Per-component bounds for a deterministic random 3D vector source.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Vec3Range {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Vec3Range {
    pub const fn new(min: [f32; 3], max: [f32; 3]) -> Self {
        Self { min, max }
    }

    pub fn sample(self, random: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|axis| {
            self.min[axis] + (self.max[axis] - self.min[axis]) * random[axis].clamp(0.0, 1.0)
        })
    }
}

/// Three independently editable scalar curves evaluated in one shared domain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Vec3Curve {
    pub curves: [Curve; 3],
}

impl Vec3Curve {
    pub fn constant(value: [f32; 3]) -> Self {
        Self {
            curves: std::array::from_fn(|axis| {
                Curve::normalized(
                    vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 0.0)],
                    ScalarRange::new(value[axis], value[axis]),
                )
            }),
        }
    }

    pub fn sample(&self, time: f32) -> [f32; 3] {
        std::array::from_fn(|axis| self.curves[axis].sample(time))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Curve {
    pub id: CurveId,
    pub keys: Vec<CurveKey>,
    /// Maps normalized key values into authored output units. Missing on legacy curves whose keys
    /// already contain output values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_range: Option<ScalarRange>,
}

impl Curve {
    pub fn new(keys: Vec<CurveKey>) -> Self {
        Self {
            id: CurveId::new(),
            keys,
            output_range: None,
        }
    }

    /// Creates a curve whose keys describe a normalized shape and whose range provides output
    /// units.
    pub fn normalized(keys: Vec<CurveKey>, output_range: ScalarRange) -> Self {
        Self {
            id: CurveId::new(),
            keys,
            output_range: Some(output_range),
        }
    }

    /// Returns the semantic output range. Legacy absolute-valued curves derive it from their keys.
    pub fn output_range(&self) -> ScalarRange {
        self.output_range.unwrap_or_else(|| {
            let mut minimum = f32::INFINITY;
            let mut maximum = f32::NEG_INFINITY;
            for key in &self.keys {
                if key.value.is_finite() {
                    minimum = minimum.min(key.value);
                    maximum = maximum.max(key.value);
                }
            }
            if minimum.is_finite() && maximum.is_finite() {
                ScalarRange::new(minimum, maximum)
            } else {
                ScalarRange::new(0.0, 0.0)
            }
        })
    }

    /// Converts a legacy absolute-valued curve to normalized shape keys without changing output.
    pub fn normalize_output(&mut self) {
        if self.output_range.is_some() {
            return;
        }
        let range = self.output_range();
        let span = range.max - range.min;
        for key in &mut self.keys {
            key.value = if span.abs() <= f32::EPSILON {
                0.0
            } else {
                ((key.value - range.min) / span).clamp(0.0, 1.0)
            };
        }
        self.output_range = Some(range);
    }

    /// Maps a stored key value to the curve's authored output units.
    pub fn output_value(&self, value: f32) -> f32 {
        self.output_range.map_or(value, |range| range.sample(value))
    }

    pub fn sample(&self, time: f32) -> f32 {
        let Some(first) = self.keys.first() else {
            return 0.0;
        };
        let time = time.clamp(0.0, 1.0);
        let value = if time <= first.time {
            first.value
        } else {
            let mut sampled = None;
            for pair in self.keys.windows(2) {
                let (a, b) = (&pair[0], &pair[1]);
                if time <= b.time {
                    let span = (b.time - a.time).max(f32::EPSILON);
                    let x = ((time - a.time) / span).clamp(0.0, 1.0);
                    let smooth = x * x * (3.0 - 2.0 * x);
                    sampled = Some(a.value + (b.value - a.value) * smooth);
                    break;
                }
            }
            sampled.unwrap_or_else(|| self.keys.last().map_or(0.0, |key| key.value))
        };
        self.output_value(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CurveKey {
    pub time: f32,
    pub value: f32,
}

impl CurveKey {
    pub const fn new(time: f32, value: f32) -> Self {
        Self { time, value }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gradient {
    pub id: GradientId,
    pub keys: Vec<ColorKey>,
}

impl Gradient {
    pub fn new(keys: Vec<ColorKey>) -> Self {
        Self {
            id: GradientId::new(),
            keys,
        }
    }

    pub fn sample(&self, time: f32) -> [f32; 4] {
        let Some(first) = self.keys.first() else {
            return [1.0; 4];
        };
        let time = time.clamp(0.0, 1.0);
        if time <= first.time {
            return first.color;
        }
        for pair in self.keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if time <= b.time {
                let x = ((time - a.time) / (b.time - a.time).max(f32::EPSILON)).clamp(0.0, 1.0);
                return std::array::from_fn(|index| {
                    a.color[index] + (b.color[index] - a.color[index]) * x
                });
            }
        }
        self.keys.last().map_or([1.0; 4], |key| key.color)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ColorKey {
    pub time: f32,
    pub color: [f32; 4],
}

impl ColorKey {
    pub const fn new(time: f32, color: [f32; 4]) -> Self {
        Self { time, color }
    }
}

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("could not read or write the effect asset: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse the effect asset: {0}")]
    Parse(#[from] ron::error::SpannedError),
    #[error("could not serialize the effect asset: {0}")]
    Serialize(#[from] ron::Error),
    #[error("effect format version {found} is unsupported; expected {current}")]
    UnsupportedFormat { found: u32, current: u32 },
    #[error("could not migrate effect format {from} to {to}: {message}")]
    Migration { from: u32, to: u32, message: String },
    #[error("effect validation failed: {0}")]
    Validation(#[from] ValidationReport),
}

fn register_id(
    report: &mut ValidationReport,
    semantic_ids: &mut BTreeMap<u128, String>,
    id: u128,
    path: String,
) {
    if id == 0 {
        report.push(Diagnostic::error(
            DiagnosticCode::NilId,
            &path,
            "semantic ID cannot be nil",
        ));
    }
    if let Some(previous) = semantic_ids.insert(id, path.clone()) {
        report.push(Diagnostic::error(
            DiagnosticCode::DuplicateId,
            path,
            format!("semantic ID is already used at {previous}"),
        ));
    }
}

fn invalid_value(report: &mut ValidationReport, path: &str, message: &str) {
    report.push(Diagnostic::error(
        DiagnosticCode::InvalidValue,
        path,
        message,
    ));
}

fn validate_range(range: ScalarRange, path: &str, field: &str, report: &mut ValidationReport) {
    if !range.min.is_finite() || !range.max.is_finite() || range.min > range.max {
        invalid_value(report, path, &format!("{field} range is invalid"));
    }
}

fn validate_curve(curve: &Curve, path: &str, report: &mut ValidationReport) {
    if curve.keys.is_empty()
        || curve.keys.iter().any(|key| {
            !key.time.is_finite() || !key.value.is_finite() || !(0.0..=1.0).contains(&key.time)
        })
        || curve
            .keys
            .windows(2)
            .any(|pair| pair[0].time > pair[1].time)
    {
        invalid_value(report, path, "curve keys are invalid or empty");
    }
    if let Some(range) = curve.output_range {
        validate_range(range, path, "curve output", report);
        if curve
            .keys
            .iter()
            .any(|key| !(0.0..=1.0).contains(&key.value))
        {
            invalid_value(
                report,
                path,
                "normalized curve key values must be between zero and one",
            );
        }
    }
}

fn validate_gradient(gradient: &Gradient, path: &str, report: &mut ValidationReport) {
    if gradient.keys.is_empty()
        || gradient.keys.iter().any(|key| {
            !key.time.is_finite()
                || !(0.0..=1.0).contains(&key.time)
                || key.color.iter().any(|channel| !channel.is_finite())
        })
        || gradient
            .keys
            .windows(2)
            .any(|pair| pair[0].time > pair[1].time)
    {
        invalid_value(report, path, "gradient keys are invalid or empty");
    }
}

fn validate_value(
    value: &Value,
    path: &str,
    report: &mut ValidationReport,
    semantic_ids: &mut BTreeMap<u128, String>,
) {
    match value {
        Value::Scalar(value) if !value.is_finite() => {
            invalid_value(report, path, "scalar value must be finite");
        }
        Value::Vec2(value) if value.iter().any(|item| !item.is_finite()) => {
            invalid_value(report, path, "vector value must be finite");
        }
        Value::Vec3(value) if value.iter().any(|item| !item.is_finite()) => {
            invalid_value(report, path, "vector value must be finite");
        }
        Value::Vec3Range(range) => {
            for axis in 0..3 {
                validate_range(
                    ScalarRange::new(range.min[axis], range.max[axis]),
                    &format!("{path}.{axis}"),
                    "vector range",
                    report,
                );
            }
        }
        Value::Vec3Curve(curve) => {
            for (axis, value) in curve.curves.iter().enumerate() {
                let axis_path = format!("{path}.curves[{axis}]");
                validate_curve(value, &axis_path, report);
                register_id(
                    report,
                    semantic_ids,
                    value.id.as_uuid().as_u128(),
                    format!("{axis_path}.id"),
                );
            }
        }
        Value::Vec4(value) if value.iter().any(|item| !item.is_finite()) => {
            invalid_value(report, path, "vector value must be finite");
        }
        Value::Range(range) => validate_range(*range, path, "value", report),
        Value::Curve(curve) => {
            validate_curve(curve, path, report);
            register_id(
                report,
                semantic_ids,
                curve.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
        }
        Value::Gradient(gradient) => {
            validate_gradient(gradient, path, report);
            register_id(
                report,
                semantic_ids,
                gradient.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
        }
        Value::Shape(shape) => match shape {
            EmitterShape::Circle { radius }
            | EmitterShape::Ring { radius }
            | EmitterShape::Sphere { radius }
            | EmitterShape::Hemisphere { radius }
                if !radius.is_finite() || *radius < 0.0 =>
            {
                invalid_value(report, path, "shape radius must be finite and non-negative");
            }
            EmitterShape::Cone { radius, depth } | EmitterShape::Cylinder { radius, depth }
                if !radius.is_finite() || *radius < 0.0 || !depth.is_finite() || *depth <= 0.0 =>
            {
                invalid_value(
                    report,
                    path,
                    "shape radius must be non-negative and depth must be positive",
                );
            }
            EmitterShape::Box { half_extents }
                if half_extents
                    .iter()
                    .any(|extent| !extent.is_finite() || *extent <= 0.0) =>
            {
                invalid_value(report, path, "box half-extents must be finite and positive");
            }
            _ => {}
        },
        Value::Parameter(id) if id.is_nil() => {
            invalid_value(report, path, "parameter reference cannot be nil");
        }
        Value::Asset(id) if id.is_nil() => {
            invalid_value(report, path, "asset reference cannot be nil");
        }
        Value::Material(id) if id.is_nil() => {
            invalid_value(report, path, "material reference cannot be nil");
        }
        _ => {}
    }
}
