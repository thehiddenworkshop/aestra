//! Ordered sRGB swatches for project material palettes.

use thiserror::Error;

pub const MAX_PALETTE_COLORS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteAsset {
    colors: Vec<[u8; 3]>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PaletteError {
    #[error("Expected a GIMP Palette header")]
    InvalidHeader,
    #[error("Invalid RGB swatch on line {0}")]
    InvalidSwatch(usize),
    #[error("A palette must have between 1 and {MAX_PALETTE_COLORS} colors")]
    InvalidColorCount,
}

impl PaletteAsset {
    pub fn new(colors: Vec<[u8; 3]>) -> Result<Self, PaletteError> {
        if colors.is_empty() || colors.len() > MAX_PALETTE_COLORS {
            return Err(PaletteError::InvalidColorCount);
        }
        Ok(Self { colors })
    }

    pub fn colors(&self) -> &[[u8; 3]] {
        &self.colors
    }

    pub fn from_gpl(source: &str) -> Result<Self, PaletteError> {
        let mut lines = source.trim_start_matches('\u{feff}').lines().enumerate();
        if !matches!(lines.next(), Some((_, header)) if header.trim() == "GIMP Palette") {
            return Err(PaletteError::InvalidHeader);
        }
        let mut colors = Vec::new();
        for (index, line) in lines {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with("Name:")
                || line.starts_with("Columns:")
            {
                continue;
            }
            let mut components = line.split_whitespace();
            let mut color = [0; 3];
            for channel in &mut color {
                *channel = components
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or(PaletteError::InvalidSwatch(index + 1))?;
            }
            if colors.len() == MAX_PALETTE_COLORS {
                return Err(PaletteError::InvalidColorCount);
            }
            colors.push(color);
        }
        Self::new(colors)
    }
}
