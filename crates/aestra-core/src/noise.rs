//! Versioned procedural-field contract, independent of a renderer or editor.
use crate::{MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId};
use serde::{Deserialize, Serialize};

pub const MAX_NOISE_OCTAVES: u32 = 8;
pub const MAX_MATERIAL_NOISE_SEED: u32 = 0x00ff_ffff;
/// Coordinates repeat after this many lattice cells; negative cells use floor.
pub const NOISE_PERIOD: f32 = 65_536.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoiseBasis {
    Value,
    Gradient,
}

/// Built-in material functions use stable v1 IDs. Changing their numerical
/// contract requires new IDs rather than silently changing saved effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseFunction {
    pub basis: NoiseBasis,
    pub dimensions: u8,
    pub fractal: bool,
}
impl NoiseFunction {
    pub const ALL: [Self; 8] = [
        Self {
            basis: NoiseBasis::Value,
            dimensions: 2,
            fractal: false,
        },
        Self {
            basis: NoiseBasis::Value,
            dimensions: 3,
            fractal: false,
        },
        Self {
            basis: NoiseBasis::Gradient,
            dimensions: 2,
            fractal: false,
        },
        Self {
            basis: NoiseBasis::Gradient,
            dimensions: 3,
            fractal: false,
        },
        Self {
            basis: NoiseBasis::Value,
            dimensions: 2,
            fractal: true,
        },
        Self {
            basis: NoiseBasis::Value,
            dimensions: 3,
            fractal: true,
        },
        Self {
            basis: NoiseBasis::Gradient,
            dimensions: 2,
            fractal: true,
        },
        Self {
            basis: NoiseBasis::Gradient,
            dimensions: 3,
            fractal: true,
        },
    ];
    pub fn id(self) -> MaterialFunctionId {
        let index = Self::ALL
            .iter()
            .position(|k| *k == self)
            .expect("supported noise function");
        MaterialFunctionId::from_u128(0xa3574a0000004000800000000000f100 + index as u128)
    }
    pub fn from_id(id: MaterialFunctionId) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
    pub fn name(self) -> String {
        format!(
            "{}{} Noise {}D",
            if self.fractal { "fBm " } else { "" },
            match self.basis {
                NoiseBasis::Value => "Value",
                NoiseBasis::Gradient => "Gradient",
            },
            self.dimensions
        )
    }
}

pub const fn input(index: u128) -> MaterialFunctionInputId {
    MaterialFunctionInputId::from_u128(0xa3574a0000004000800000000000f200 + index)
}
pub const OUTPUT: MaterialFunctionOutputId =
    MaterialFunctionOutputId::from_u128(0xa3574a0000004000800000000000f300);
pub const POSITION: MaterialFunctionInputId = input(0);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseSettings {
    pub basis: NoiseBasis,
    pub dimensions: u8,
    pub scale: f32,
    pub offset: [f32; 3],
    pub amplitude: f32,
    pub seed: u32,
    pub octaves: u32,
    pub lacunarity: f32,
    pub gain: f32,
    /// Coordinate-space pixel/sample footprint, before scale. Zero is unfiltered.
    /// Explicit on compute/vertex paths; fragment callers may supply derivatives.
    pub footprint: f32,
}
impl Default for NoiseSettings {
    fn default() -> Self {
        Self {
            basis: NoiseBasis::Value,
            dimensions: 3,
            scale: 4.0,
            offset: [0.0; 3],
            amplitude: 1.0,
            seed: 0,
            octaves: 1,
            lacunarity: 2.0,
            gain: 0.5,
            footprint: 0.0,
        }
    }
}
