use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! semantic_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub const fn from_u128(value: u128) -> Self {
                Self(Uuid::from_u128(value))
            }

            pub const fn is_nil(self) -> bool {
                self.0.is_nil()
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

semantic_id!(EffectId);
semantic_id!(EffectClipId);
semantic_id!(MarkerId);
semantic_id!(ChoreographyEventId);
semantic_id!(EmitterId);
semantic_id!(EmitterRegionId);
semantic_id!(ModuleId);
semantic_id!(RendererId);
semantic_id!(CurveId);
semantic_id!(GradientId);
semantic_id!(ParameterId);
semantic_id!(BindingId);
semantic_id!(EventId);
semantic_id!(AssetId);
semantic_id!(MaterialId);
semantic_id!(MaterialProgramId);
semantic_id!(MaterialParameterId);
semantic_id!(MaterialExpressionId);
semantic_id!(MaterialFunctionId);
semantic_id!(MaterialFunctionInputId);
semantic_id!(MaterialFunctionOutputId);
semantic_id!(MaterialPresetId);
semantic_id!(StageId);

impl StageId {
    /// A deterministic stage id derived from a name (extensible-stages M3). Simulation stages are
    /// identified by their authored name in the flat in-memory model, so their `StageId` is derived
    /// from that name rather than randomly generated — the same name always yields the same id, so a
    /// v4 round trip is stable without the flat model storing a separate per-stage UUID.
    pub fn for_name(name: &str) -> Self {
        // Two FNV-1a passes with distinct offsets fill the 128 bits deterministically (no deps, stable
        // across platforms).
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let fnv = |seed: u64| -> u64 {
            let mut hash = seed;
            for byte in name.as_bytes() {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(FNV_PRIME);
            }
            hash
        };
        let high = fnv(FNV_OFFSET);
        let low = fnv(FNV_OFFSET ^ 0x9E37_79B9_7F4A_7C15);
        Self::from_u128((u128::from(high) << 64) | u128::from(low))
    }
}
