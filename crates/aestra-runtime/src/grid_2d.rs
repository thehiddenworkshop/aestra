//! Effect-space XY grids backed by the existing staged field ABI.

use crate::{FieldLayout, StagedDispatch};
use aestra_core::ResourceTypeId;
use std::num::NonZeroU32;

/// Number of floating-point components per cell. Three-component cells have vec4 stride.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grid2dChannels {
    Scalar,
    Vector2,
    Vector3,
    Vector4,
}

impl Grid2dChannels {
    fn components(self) -> u32 {
        match self {
            Self::Scalar => 1,
            Self::Vector2 => 2,
            Self::Vector3 => 3,
            Self::Vector4 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Grid2dLayoutError {
    #[error("Grid2D dimensions must be nonzero and fit in a 32-bit cell index")]
    InvalidDimensions,
    #[error("Grid2D needs positive cell size and finite, increasing bounds on both axes")]
    InvalidBounds,
}

/// A dense Grid2D domain with square, cell-centered cells, X varying fastest.
/// Its fields share the staged storage format and lie on effect-space Z = 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid2dLayout {
    dimensions: [u32; 2],
    origin: [f32; 2],
    cell_size: f32,
}

impl Grid2dLayout {
    /// `origin` is the minimum corner of the first cell in effect-space XY.
    pub fn new(
        dimensions: [u32; 2],
        origin: [f32; 2],
        cell_size: f32,
    ) -> Result<Self, Grid2dLayoutError> {
        if dimensions[0]
            .checked_mul(dimensions[1])
            .is_none_or(|cells| cells == 0)
        {
            return Err(Grid2dLayoutError::InvalidDimensions);
        }
        if !(cell_size.is_finite() && cell_size > 0.0)
            || !origin.into_iter().zip(dimensions).all(|(low, count)| {
                let high = low + count as f32 * cell_size;
                high.is_finite() && high > low
            })
        {
            return Err(Grid2dLayoutError::InvalidBounds);
        }
        Ok(Self {
            dimensions,
            origin,
            cell_size,
        })
    }

    /// Declares one named channel buffer in the grid.
    pub fn field(self, resource: ResourceTypeId, channels: Grid2dChannels) -> FieldLayout {
        let [width, height] = self.dimensions;
        FieldLayout {
            resource,
            dims: [width, height, 1],
            components: channels.components(),
            origin: [self.origin[0], self.origin[1], -self.cell_size * 0.5],
            cell_size: self.cell_size,
            staggered: false,
            bricks: None,
        }
    }

    /// Covers the grid with 2D workgroups. Kernels guard invocations past the last cell.
    pub fn dispatch(self, [x, y]: [NonZeroU32; 2]) -> StagedDispatch {
        StagedDispatch {
            x: self.dimensions[0].div_ceil(x.get()),
            y: self.dimensions[1].div_ceil(y.get()),
            z: 1,
        }
    }
}
