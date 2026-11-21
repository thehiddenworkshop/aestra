//! Reference sampling for binary ordered coverage shared by material previews.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BayerPattern {
    TwoByTwo,
    #[default]
    FourByFour,
    EightByEight,
}

impl BayerPattern {
    pub const fn levels(self) -> u32 {
        match self {
            Self::TwoByTwo => 1,
            Self::FourByFour => 2,
            Self::EightByEight => 3,
        }
    }

    pub const fn width(self) -> u32 {
        1 << self.levels()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DitherPattern {
    Bayer(BayerPattern),
    BlueNoise,
}

impl DitherPattern {
    pub fn sample(self, alpha: f32, position: [f32; 2], cell_size: f32) -> f32 {
        match self {
            Self::Bayer(pattern) => bayer(pattern, alpha, position, cell_size),
            Self::BlueNoise => {
                let [x, y] = tile_cell(position, cell_size, BLUE_NOISE_WIDTH);
                let rank = BLUE_NOISE_RANKS[y as usize * BLUE_NOISE_WIDTH as usize + x as usize];
                binary_coverage(alpha, u32::from(rank), BLUE_NOISE_RANKS.len() as u32)
            }
        }
    }
}

pub const BLUE_NOISE_WIDTH: u32 = 32;
pub const BLUE_NOISE_RANKS: [u16; (BLUE_NOISE_WIDTH * BLUE_NOISE_WIDTH) as usize] =
    include!("blue_noise_ranks.in");

/// Samples binary Bayer coverage in the supplied coordinate space.
pub fn bayer(pattern: BayerPattern, alpha: f32, position: [f32; 2], cell_size: f32) -> f32 {
    let [x, y] = tile_cell(position, cell_size, pattern.width());
    let mut rank = 0;
    for bit in 0..pattern.levels() {
        let [x, y] = [(x >> bit) & 1, (y >> bit) & 1];
        rank = 4 * rank + 2 * (x ^ y) + y;
    }
    binary_coverage(alpha, rank as u32, pattern.width() * pattern.width())
}

fn tile_cell(position: [f32; 2], cell_size: f32, width: u32) -> [i32; 2] {
    let size = if cell_size.is_finite() && cell_size > 0.0 {
        cell_size
    } else {
        1.0
    };
    // Match the shader's float-to-integer range, including very large positions.
    let tile = width as i32 - 1;
    position.map(|value| ((value / size).floor().clamp(-16_777_216.0, 16_777_216.0) as i32) & tile)
}

fn binary_coverage(alpha: f32, rank: u32, area: u32) -> f32 {
    if alpha > (rank as f32 + 0.5) / area as f32 {
        1.0
    } else {
        0.0
    }
}

/// Samples one Bayer 4×4 cell in the supplied coordinate space.
pub fn bayer_4x4(alpha: f32, position: [f32; 2], cell_size: f32) -> f32 {
    bayer(BayerPattern::FourByFour, alpha, position, cell_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blue_noise_has_unique_ranks_and_tiles_at_negative_coordinates() {
        let width = BLUE_NOISE_WIDTH as usize;
        let area = BLUE_NOISE_RANKS.len();
        let mut ranks = BLUE_NOISE_RANKS;
        ranks.sort_unstable();
        assert!(ranks.into_iter().map(usize::from).eq(0..area));

        for (index, rank) in BLUE_NOISE_RANKS.into_iter().enumerate() {
            let position = [index % width, index / width].map(|cell| 2.0 * cell as f32 + 0.5);
            let wrapped = position.map(|coordinate| coordinate - 2.0 * width as f32);
            let threshold = (f32::from(rank) + 0.5) / area as f32;
            let above = threshold + 0.25 / area as f32;
            for sample in [position, wrapped] {
                assert_eq!(DitherPattern::BlueNoise.sample(threshold, sample, 2.0), 0.0);
                assert_eq!(DitherPattern::BlueNoise.sample(above, sample, 2.0), 1.0);
            }
        }
    }

    #[test]
    fn blue_noise_masks_suppress_low_frequency_energy() {
        let width = BLUE_NOISE_WIDTH as usize;
        let area = BLUE_NOISE_RANKS.len();
        for coverage in [area / 8, area / 4, area / 2, 3 * area / 4, 7 * area / 8] {
            let white_noise_power = (coverage * (area - coverage)) as f64 / (area - 1) as f64;
            for (fx, fy) in [(1, 0), (0, 1), (1, 1), (1, -1), (2, 0), (0, 2)] {
                let (mut real, mut imaginary) = (0.0, 0.0);
                for (index, rank) in BLUE_NOISE_RANKS.iter().copied().enumerate() {
                    if usize::from(rank) < coverage {
                        let phase = (fx * (index % width) as i32 + fy * (index / width) as i32)
                            as f64
                            * std::f64::consts::TAU
                            / width as f64;
                        let (sin, cos) = phase.sin_cos();
                        real += cos;
                        imaginary += sin;
                    }
                }
                assert!(
                    real * real + imaginary * imaginary < 0.2 * white_noise_power,
                    "coverage {coverage}/{area}, frequency ({fx}, {fy})",
                );
            }
        }
    }

    #[test]
    fn ordered_mask_uses_all_bayer_ranks_and_repeats_across_negative_cells() {
        let ranks = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
        for (y, row) in ranks.into_iter().enumerate() {
            for (x, rank) in row.into_iter().enumerate() {
                let position = [x as f32 + 0.5, y as f32 + 0.5];
                let negative = position.map(|coordinate| coordinate - 4.0);
                let threshold = (rank as f32 + 0.5) / 16.0;
                assert_eq!(bayer_4x4(threshold, position, 1.0), 0.0);
                assert_eq!(bayer_4x4(threshold + 0.001, position, 1.0), 1.0);
                assert_eq!(bayer_4x4(threshold, negative, 1.0), 0.0);
                assert_eq!(bayer_4x4(threshold + 0.001, negative, 1.0), 1.0);
            }
        }
    }

    #[test]
    fn new_tile_sizes_keep_rank_order_and_respect_cell_size() {
        let bayer_2 = [[0, 2], [3, 1]];
        let bayer_4 = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
        for pattern in [BayerPattern::TwoByTwo, BayerPattern::EightByEight] {
            let width = pattern.width() as usize;
            for y in 0..width {
                for x in 0..width {
                    let rank = if width == 2 {
                        bayer_2[y][x]
                    } else {
                        4 * bayer_4[y % 4][x % 4] + bayer_2[y / 4][x / 4]
                    };
                    let threshold = (rank as f32 + 0.5) / (width * width) as f32;
                    let position = [x, y].map(|cell| 2.0 * cell as f32 + 0.5);
                    for sample in [
                        position,
                        position.map(|coordinate| coordinate - 2.0 * width as f32),
                    ] {
                        assert_eq!(bayer(pattern, threshold, sample, 2.0), 0.0);
                        assert_eq!(bayer(pattern, threshold + 0.001, sample, 2.0), 1.0);
                    }
                }
            }
        }
    }
}
