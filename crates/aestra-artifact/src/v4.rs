//! Artifact v4 additions: compiled host bindings and clip binding forwards (host bindings HB2), and
//! plugin extension stages with their Execution IR (extensible-stages M10, deferred to this bump).
//! Explicit DTOs, as everywhere in this crate — never the runtime structs themselves.

use crate::{ArtifactError, encode_u32, invalid};
use aestra_core::{
    BindingFieldId, BindingId, BindingKindId, BindingUpdateMode, ModuleId, ModuleTypeId,
    PropertyBag, ResourceTypeId, StageId, StageTypeId, ValueType,
};
use aestra_runtime::{
    BindingLayout, BindingSlot, CompiledBinding, CompiledBindingField, CompiledBindingForward,
    CompiledExtensionStage, ComputeOp, CopyOp, ExecutionBlock, ExecutionOp, ExtensionModulePlan,
    RepeatPolicy, ResourceAccess, ResourceAccessMode, ResourceDescriptor, ResourceLifetime,
    StagedDispatch,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BindingV4 {
    source: BindingId,
    name: String,
    kind: BindingKindId,
    update_mode: BindingUpdateMode,
    required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    required_fields: Vec<BindingFieldId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    optional_fields: Vec<BindingFieldId>,
    layout: Vec<BindingFieldLayoutV4>,
    stride: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct BindingFieldLayoutV4 {
    field: BindingFieldId,
    value_type: ValueType,
    offset: u32,
}

impl From<&CompiledBinding> for BindingV4 {
    fn from(binding: &CompiledBinding) -> Self {
        Self {
            source: binding.source,
            name: binding.name.clone(),
            kind: binding.kind.clone(),
            update_mode: binding.update_mode,
            required: binding.required,
            required_fields: binding.required_fields.iter().cloned().collect(),
            optional_fields: binding.optional_fields.iter().cloned().collect(),
            layout: binding
                .layout
                .fields
                .iter()
                .map(|field| BindingFieldLayoutV4 {
                    field: field.field.clone(),
                    value_type: field.value_type,
                    offset: field.offset,
                })
                .collect(),
            stride: binding.layout.stride,
        }
    }
}

impl BindingV4 {
    pub(crate) fn decode(self, index: usize) -> Result<CompiledBinding, ArtifactError> {
        let path = format!("effect.bindings[{index}]");
        if self.name.trim().is_empty() {
            return invalid(format!("{path}.name"), "binding name cannot be empty");
        }
        let binding = CompiledBinding {
            source: self.source,
            name: self.name,
            kind: self.kind,
            update_mode: self.update_mode,
            required: self.required,
            required_fields: self.required_fields.into_iter().collect(),
            optional_fields: self.optional_fields.into_iter().collect(),
            layout: BindingLayout {
                fields: self
                    .layout
                    .into_iter()
                    .map(|field| CompiledBindingField {
                        field: field.field,
                        value_type: field.value_type,
                        offset: field.offset,
                    })
                    .collect(),
                stride: self.stride,
            },
        };
        match binding.validate() {
            Ok(()) => Ok(binding),
            Err(message) => invalid(format!("{path}.layout"), message),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BindingForwardV4 {
    child: BindingId,
    child_slot: u32,
    parent_slot: u32,
}

impl BindingForwardV4 {
    pub(crate) fn encode(
        forward: &CompiledBindingForward,
        path: &str,
    ) -> Result<Self, ArtifactError> {
        Ok(Self {
            child: forward.child,
            child_slot: encode_u32(forward.child_slot.0, format!("{path}.child_slot"))?,
            parent_slot: encode_u32(forward.parent_slot.0, format!("{path}.parent_slot"))?,
        })
    }

    /// The parent slot is validated against the owning effect; the child slot against the child
    /// effect, which lives in another artifact, when the project is assembled.
    pub(crate) fn decode(
        self,
        path: &str,
        parent_bindings: usize,
    ) -> Result<CompiledBindingForward, ArtifactError> {
        if self.parent_slot as usize >= parent_bindings {
            return invalid(
                format!("{path}.parent_slot"),
                format!(
                    "slot {} is out of range for {parent_bindings} bindings",
                    self.parent_slot
                ),
            );
        }
        Ok(CompiledBindingForward {
            child: self.child,
            child_slot: BindingSlot(self.child_slot as usize),
            parent_slot: BindingSlot(self.parent_slot as usize),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ExtensionStageV4 {
    id: StageId,
    stage_type: StageTypeId,
    name: String,
    modules: Vec<ExtensionModulePlanV4>,
    block: ExecutionBlockV4,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExtensionModulePlanV4 {
    source: ModuleId,
    module_type: ModuleTypeId,
    entry_point: String,
    #[serde(default, skip_serializing_if = "PropertyBag::is_empty")]
    parameters: PropertyBag,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExecutionBlockV4 {
    resources: Vec<ResourceDescriptorV4>,
    ops: Vec<ExecutionOpV4>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResourceDescriptorV4 {
    id: ResourceTypeId,
    bytes: u64,
    lifetime: ResourceLifetimeV4,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ResourceLifetimeV4 {
    Persistent,
    Transient,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum AccessModeV4 {
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResourceAccessV4 {
    resource: ResourceTypeId,
    mode: AccessModeV4,
}

#[derive(Debug, Serialize, Deserialize)]
enum ExecutionOpV4 {
    Compute {
        name: String,
        entry_point: String,
        accesses: Vec<ResourceAccessV4>,
        dispatch: (u32, u32, u32),
    },
    Barrier,
    Copy {
        from: ResourceTypeId,
        to: ResourceTypeId,
    },
    Repeat {
        count: u32,
        body: Vec<ExecutionOpV4>,
    },
}

impl From<&ExecutionOp> for ExecutionOpV4 {
    fn from(op: &ExecutionOp) -> Self {
        match op {
            ExecutionOp::Compute(compute) => Self::Compute {
                name: compute.name.clone(),
                entry_point: compute.entry_point.clone(),
                accesses: compute
                    .accesses
                    .iter()
                    .map(|access| ResourceAccessV4 {
                        resource: access.resource.clone(),
                        mode: match access.mode {
                            ResourceAccessMode::Read => AccessModeV4::Read,
                            ResourceAccessMode::Write => AccessModeV4::Write,
                            ResourceAccessMode::ReadWrite => AccessModeV4::ReadWrite,
                        },
                    })
                    .collect(),
                dispatch: (compute.dispatch.x, compute.dispatch.y, compute.dispatch.z),
            },
            ExecutionOp::Barrier => Self::Barrier,
            ExecutionOp::Copy(copy) => Self::Copy {
                from: copy.from.clone(),
                to: copy.to.clone(),
            },
            ExecutionOp::Repeat { policy, body } => Self::Repeat {
                count: policy.count(),
                body: body.iter().map(Self::from).collect(),
            },
        }
    }
}

impl From<ExecutionOpV4> for ExecutionOp {
    fn from(op: ExecutionOpV4) -> Self {
        match op {
            ExecutionOpV4::Compute {
                name,
                entry_point,
                accesses,
                dispatch: (x, y, z),
            } => Self::Compute(ComputeOp {
                name,
                entry_point,
                accesses: accesses
                    .into_iter()
                    .map(|access| ResourceAccess {
                        resource: access.resource,
                        mode: match access.mode {
                            AccessModeV4::Read => ResourceAccessMode::Read,
                            AccessModeV4::Write => ResourceAccessMode::Write,
                            AccessModeV4::ReadWrite => ResourceAccessMode::ReadWrite,
                        },
                    })
                    .collect(),
                dispatch: StagedDispatch { x, y, z },
            }),
            ExecutionOpV4::Barrier => Self::Barrier,
            ExecutionOpV4::Copy { from, to } => Self::Copy(CopyOp { from, to }),
            ExecutionOpV4::Repeat { count, body } => Self::Repeat {
                policy: RepeatPolicy::FixedCount(count),
                body: body.into_iter().map(Self::from).collect(),
            },
        }
    }
}

impl From<&CompiledExtensionStage> for ExtensionStageV4 {
    fn from(stage: &CompiledExtensionStage) -> Self {
        Self {
            id: stage.id,
            stage_type: stage.stage_type.clone(),
            name: stage.name.clone(),
            modules: stage
                .modules
                .iter()
                .map(|module| ExtensionModulePlanV4 {
                    source: module.source,
                    module_type: module.module_type.clone(),
                    entry_point: module.entry_point.clone(),
                    parameters: module.parameters.clone(),
                })
                .collect(),
            block: ExecutionBlockV4 {
                resources: stage
                    .block
                    .resources
                    .iter()
                    .map(|resource| ResourceDescriptorV4 {
                        id: resource.id.clone(),
                        bytes: resource.bytes,
                        lifetime: match resource.lifetime {
                            ResourceLifetime::Persistent => ResourceLifetimeV4::Persistent,
                            ResourceLifetime::Transient => ResourceLifetimeV4::Transient,
                        },
                    })
                    .collect(),
                ops: stage.block.ops.iter().map(ExecutionOpV4::from).collect(),
            },
        }
    }
}

impl ExtensionStageV4 {
    pub(crate) fn decode(self, path: &str) -> Result<CompiledExtensionStage, ArtifactError> {
        let block = ExecutionBlock {
            resources: self
                .block
                .resources
                .into_iter()
                .map(|resource| ResourceDescriptor {
                    id: resource.id,
                    bytes: resource.bytes,
                    lifetime: match resource.lifetime {
                        ResourceLifetimeV4::Persistent => ResourceLifetime::Persistent,
                        ResourceLifetimeV4::Transient => ResourceLifetime::Transient,
                    },
                })
                .collect(),
            ops: self.block.ops.into_iter().map(ExecutionOp::from).collect(),
        };
        if let Err(error) = block.validate() {
            return invalid(format!("{path}.block"), error.to_string());
        }
        Ok(CompiledExtensionStage {
            id: self.id,
            stage_type: self.stage_type,
            name: self.name,
            modules: self
                .modules
                .into_iter()
                .map(|module| ExtensionModulePlan {
                    source: module.source,
                    module_type: module.module_type,
                    entry_point: module.entry_point,
                    parameters: module.parameters,
                })
                .collect(),
            block,
        })
    }
}
