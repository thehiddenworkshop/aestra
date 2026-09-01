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
semantic_id!(EventId);
semantic_id!(AssetId);
semantic_id!(MaterialId);
semantic_id!(MaterialProgramId);
semantic_id!(MaterialParameterId);
semantic_id!(MaterialExpressionId);
